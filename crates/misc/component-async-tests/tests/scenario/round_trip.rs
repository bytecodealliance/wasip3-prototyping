use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::fs;
use wasmtime::component::{Component, Linker, PromisesUnordered, ResourceTable, Val};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::Ctx;

pub use component_async_tests::util::{annotate, compose, init_logger};

#[tokio::test]
pub async fn async_round_trip_stackful() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_synchronous() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_wait() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_stackless_plus_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(stackless, stackless).await
}

#[tokio::test]
async fn async_round_trip_synchronous_plus_stackless() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(synchronous, stackless).await
}

#[tokio::test]
async fn async_round_trip_stackless_plus_synchronous() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(stackless, synchronous).await
}

#[tokio::test]
async fn async_round_trip_synchronous_plus_synchronous() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(synchronous, synchronous).await
}

#[tokio::test]
async fn async_round_trip_wait_plus_wait() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    test_round_trip_composed(wait, wait).await
}

#[tokio::test]
async fn async_round_trip_synchronous_plus_wait() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    test_round_trip_composed(synchronous, wait).await
}

#[tokio::test]
async fn async_round_trip_wait_plus_synchronous() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(wait, synchronous).await
}

#[tokio::test]
async fn async_round_trip_stackless_plus_wait() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    test_round_trip_composed(stackless, wait).await
}

#[tokio::test]
async fn async_round_trip_wait_plus_stackless() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(wait, stackless).await
}

#[tokio::test]
async fn async_round_trip_stackful_plus_stackful() -> Result<()> {
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    test_round_trip_composed(stackful, stackful).await
}

#[tokio::test]
async fn async_round_trip_stackful_plus_stackless() -> Result<()> {
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(stackful, stackless).await
}

#[tokio::test]
async fn async_round_trip_stackless_plus_stackful() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    test_round_trip_composed(stackless, stackful).await
}

#[tokio::test]
async fn async_round_trip_synchronous_plus_stackful() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    test_round_trip_composed(synchronous, stackful).await
}

#[tokio::test]
async fn async_round_trip_stackful_plus_synchronous() -> Result<()> {
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(stackful, synchronous).await
}

#[tokio::test]
pub async fn async_round_trip_stackless() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_stackless_sync_import() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_SYNC_IMPORT_COMPONENT)
            .await?,
    )
    .await
}

pub async fn test_round_trip(component: &[u8], inputs_and_outputs: &[(&str, &str)]) -> Result<()> {
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
        component_async_tests::round_trip::bindings::local::local::baz::add_to_linker_get_host(
            &mut linker,
            annotate(|ctx| ctx),
        )?;

        let mut store = make_store();

        let round_trip = component_async_tests::round_trip::bindings::RoundTrip::instantiate_async(
            &mut store, &component, &linker,
        )
        .await?;

        // Start concurrent calls and then join them all:
        let mut promises = PromisesUnordered::new();
        for (input, output) in inputs_and_outputs {
            let output = (*output).to_owned();
            promises.push(
                round_trip
                    .local_local_baz()
                    .call_foo(&mut store, (*input).to_owned())
                    .await?
                    .map(move |v| (v, output)),
            );
        }

        while let Some((actual, expected)) = promises.next(&mut store).await? {
            assert_eq!(expected, actual);
        }
    }

    // Now do it again using the dynamic API (except for WASI, where we stick with the static API):
    {
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        linker
            .root()
            .instance("local:local/baz")?
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
        let baz_instance = instance
            .get_export(&mut store, None, "local:local/baz")
            .ok_or_else(|| anyhow!("can't find `local:local/baz` in instance"))?;
        let foo_function = instance
            .get_export(&mut store, Some(&baz_instance), "foo")
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;
        let foo_function = instance
            .get_func(&mut store, foo_function)
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;

        // Start three concurrent calls and then join them all:
        let mut promises = PromisesUnordered::new();
        for (input, output) in inputs_and_outputs {
            let output = (*output).to_owned();
            promises.push(
                foo_function
                    .call_concurrent(&mut store, vec![Val::String((*input).to_owned())])
                    .await?
                    .map(move |v| (v, output)),
            );
        }

        while let Some((actual, expected)) = promises.next(&mut store).await? {
            let Some(Val::String(actual)) = actual.into_iter().next() else {
                unreachable!()
            };
            assert_eq!(expected, actual);
        }
    }

    Ok(())
}

pub async fn test_round_trip_uncomposed(component: &[u8]) -> Result<()> {
    test_round_trip(
        component,
        &[
            (
                "hello, world!",
                "hello, world! - entered guest - entered host - exited host - exited guest",
            ),
            (
                "¡hola, mundo!",
                "¡hola, mundo! - entered guest - entered host - exited host - exited guest",
            ),
            (
                "hi y'all!",
                "hi y'all! - entered guest - entered host - exited host - exited guest",
            ),
        ],
    )
    .await
}

pub async fn test_round_trip_composed(a: &[u8], b: &[u8]) -> Result<()> {
    test_round_trip(
        &compose(a, b).await?,
        &[
            (
                "hello, world!",
                "hello, world! - entered guest - entered guest - entered host \
                     - exited host - exited guest - exited guest",
            ),
            (
                "¡hola, mundo!",
                "¡hola, mundo! - entered guest - entered guest - entered host \
                     - exited host - exited guest - exited guest",
            ),
            (
                "hi y'all!",
                "hi y'all! - entered guest - entered guest - entered host \
                     - exited host - exited guest - exited guest",
            ),
        ],
    )
    .await
}
