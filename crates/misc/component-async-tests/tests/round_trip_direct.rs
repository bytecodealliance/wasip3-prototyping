use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::fs;
use wasmtime::component::{Component, Linker, PromisesUnordered, ResourceTable, Val};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::util::{annotate, init_logger};
use component_async_tests::Ctx;

#[tokio::test]
pub async fn async_direct_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_DIRECT_STACKLESS_COMPONENT).await?;
    test_round_trip_direct_uncomposed(stackless).await
}

#[tokio::test]
pub async fn async_round_trip_direct_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_DIRECT_STACKLESS_COMPONENT).await?;
    test_round_trip_direct_uncomposed(stackless).await
}

async fn test_round_trip_direct_uncomposed(component: &[u8]) -> Result<()> {
    test_round_trip_direct(
        component,
        "hello, world!",
        "hello, world! - entered guest - entered host - exited host - exited guest",
    )
    .await
}

async fn test_round_trip_direct(
    component: &[u8],
    input: &str,
    expected_output: &str,
) -> Result<()> {
    init_logger();

    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);

    let engine = Engine::new(&config)?;

    let make_store = || {
        Store::new(
            &engine,
            Ctx {
                wasi: WasiCtxBuilder::new().inherit_stdio().build(),
                table: ResourceTable::default(),
                continue_: false,
                wakers: Arc::new(Mutex::new(None)),
            },
        )
    };

    let component = Component::new(&engine, component)?;

    // First, test the `wasmtime-wit-bindgen` static API:
    {
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        component_async_tests::round_trip_direct::bindings::RoundTripDirect::add_to_linker_imports_get_host(
            &mut linker,
            annotate(|ctx| ctx),
        )?;

        let mut store = make_store();

        let round_trip =
            component_async_tests::round_trip_direct::bindings::RoundTripDirect::instantiate_async(
                &mut store, &component, &linker,
            )
            .await?;

        // Start three concurrent calls and then join them all:
        let mut promises = PromisesUnordered::new();
        for _ in 0..3 {
            promises.push(round_trip.call_foo(&mut store, input.to_owned()).await?);
        }

        while let Some(value) = promises.next(&mut store).await? {
            assert_eq!(expected_output, &value);
        }
    }

    // Now do it again using the dynamic API (except for WASI, where we stick with the static API):
    {
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        linker
            .root()
            .func_new_concurrent("foo", |_, params| async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let Some(Val::String(s)) = params.into_iter().next() else {
                    unreachable!()
                };
                Ok(vec![Val::String(format!(
                    "{s} - entered host - exited host"
                ))])
            })?;

        let mut store = make_store();

        let instance = linker.instantiate_async(&mut store, &component).await?;
        let foo_function = instance
            .get_export(&mut store, None, "foo")
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;
        let foo_function = instance
            .get_func(&mut store, foo_function)
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;

        // Start three concurrent calls and then join them all:
        let mut promises = PromisesUnordered::new();
        for _ in 0..3 {
            promises.push(
                foo_function
                    .call_concurrent(&mut store, vec![Val::String(input.to_owned())])
                    .await?,
            );
        }

        while let Some(value) = promises.next(&mut store).await? {
            let Some(Val::String(value)) = value.into_iter().next() else {
                unreachable!()
            };
            assert_eq!(expected_output, &value);
        }
    }

    Ok(())
}
