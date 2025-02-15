use std::iter;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::fs;
use wasmtime::component::{Component, Linker, PromisesUnordered, ResourceTable, Val};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

mod common;
use common::{compose, init_logger};
use component_async_tests::Ctx;

#[tokio::test]
async fn many_stackless() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn many_stackful() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn many_synchronous() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn many_wait() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn many_stackless_plus_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(stackless, stackless).await
}

#[tokio::test]
async fn many_synchronous_plus_stackless() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, stackless).await
}

#[tokio::test]
async fn many_stackless_plus_synchronous() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(stackless, synchronous).await
}

#[tokio::test]
async fn many_synchronous_plus_synchronous() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, synchronous).await
}

#[tokio::test]
async fn many_wait_plus_wait() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    test_round_trip_many_composed(wait, wait).await
}

#[tokio::test]
async fn many_synchronous_plus_wait() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, wait).await
}

#[tokio::test]
async fn many_wait_plus_synchronous() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(wait, synchronous).await
}

#[tokio::test]
async fn many_stackless_plus_wait() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    test_round_trip_many_composed(stackless, wait).await
}

#[tokio::test]
async fn many_wait_plus_stackless() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(wait, stackless).await
}

#[tokio::test]
async fn many_stackful_plus_stackful() -> Result<()> {
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    test_round_trip_many_composed(stackful, stackful).await
}

#[tokio::test]
async fn many_stackful_plus_stackless() -> Result<()> {
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(stackful, stackless).await
}

#[tokio::test]
async fn many_stackless_plus_stackful() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    test_round_trip_many_composed(stackless, stackful).await
}

#[tokio::test]
async fn many_synchronous_plus_stackful() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, stackful).await
}

#[tokio::test]
async fn many_stackful_plus_synchronous() -> Result<()> {
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(stackful, synchronous).await
}

#[tokio::test]
async fn direct_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_DIRECT_STACKLESS_COMPONENT).await?;
    test_round_trip_direct_uncomposed(stackless).await
}

// AFTER

#[tokio::test]
async fn stackless() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn stackful() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn synchronous() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn wait() -> Result<()> {
    test_round_trip_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn stackless_plus_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(stackless, stackless).await
}

#[tokio::test]
async fn synchronous_plus_stackless() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(synchronous, stackless).await
}

#[tokio::test]
async fn stackless_plus_synchronous() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(stackless, synchronous).await
}

#[tokio::test]
async fn synchronous_plus_synchronous() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(synchronous, synchronous).await
}

#[tokio::test]
async fn wait_plus_wait() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    test_round_trip_composed(wait, wait).await
}

#[tokio::test]
async fn synchronous_plus_wait() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    test_round_trip_composed(synchronous, wait).await
}

#[tokio::test]
async fn wait_plus_synchronous() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(wait, synchronous).await
}

#[tokio::test]
async fn stackless_plus_wait() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    test_round_trip_composed(stackless, wait).await
}

#[tokio::test]
async fn wait_plus_stackless() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_WAIT_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(wait, stackless).await
}

#[tokio::test]
async fn stackful_plus_stackful() -> Result<()> {
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    test_round_trip_composed(stackful, stackful).await
}

#[tokio::test]
async fn stackful_plus_stackless() -> Result<()> {
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    test_round_trip_composed(stackful, stackless).await
}

#[tokio::test]
async fn stackless_plus_stackful() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    test_round_trip_composed(stackless, stackful).await
}

#[tokio::test]
async fn synchronous_plus_stackful() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    test_round_trip_composed(synchronous, stackful).await
}

#[tokio::test]
async fn stackful_plus_synchronous() -> Result<()> {
    let stackful = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKFUL_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_composed(stackful, synchronous).await
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
        component_async_tests::round_trip::bindings::RoundTrip::add_to_linker(
            &mut linker,
            |ctx| ctx,
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

pub async fn test_round_trip_direct(
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
        component_async_tests::round_trip_direct::bindings::RoundTripDirect::add_to_linker(
            &mut linker,
            |ctx| ctx,
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

pub async fn test_round_trip_direct_uncomposed(component: &[u8]) -> Result<()> {
    test_round_trip_direct(
        component,
        "hello, world!",
        "hello, world! - entered guest - entered host - exited host - exited guest",
    )
    .await
}

pub async fn test_round_trip_many(
    component: &[u8],
    inputs_and_outputs: &[(&str, &str)],
) -> Result<()> {
    use component_async_tests::round_trip_many::bindings::exports::local::local::many;

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

    let b = 42;
    let c = vec![42u8; 42];
    let d = (4242, 424242424242);
    let e = many::Stuff {
        a: vec![42i32; 42],
        b: true,
        c: 424242,
    };
    let f = Some(e.clone());
    let g = Err(());

    // First, test the `wasmtime-wit-bindgen` static API:
    {
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        component_async_tests::round_trip_many::bindings::RoundTripMany::add_to_linker(
            &mut linker,
            |ctx| ctx,
        )?;

        let mut store = make_store();

        let round_trip_many =
            component_async_tests::round_trip_many::bindings::RoundTripMany::instantiate_async(
                &mut store, &component, &linker,
            )
            .await?;

        // Start concurrent calls and then join them all:
        let mut promises = PromisesUnordered::new();
        for (input, output) in inputs_and_outputs {
            let output = (*output).to_owned();
            promises.push(
                round_trip_many
                    .local_local_many()
                    .call_foo(
                        &mut store,
                        (*input).to_owned(),
                        b,
                        c.clone(),
                        d,
                        e.clone(),
                        f.clone(),
                        g.clone(),
                    )
                    .await?
                    .map(move |v| (v, output)),
            );
        }

        while let Some((actual, expected)) = promises.next(&mut store).await? {
            assert_eq!(
                (expected, b, c.clone(), d, e.clone(), f.clone(), g.clone()),
                actual
            );
        }
    }

    // Now do it again using the dynamic API (except for WASI, where we stick with the static API):
    {
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        linker
            .root()
            .instance("local:local/many")?
            .func_new_concurrent("foo", |_, params| async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let mut params = params.into_iter();
                let Some(Val::String(s)) = params.next() else {
                    unreachable!()
                };
                Ok(vec![Val::Tuple(
                    iter::once(Val::String(format!("{s} - entered host - exited host")))
                        .chain(params)
                        .collect(),
                )])
            })?;

        let mut store = make_store();

        let instance = linker.instantiate_async(&mut store, &component).await?;
        let baz_instance = instance
            .get_export(&mut store, None, "local:local/many")
            .ok_or_else(|| anyhow!("can't find `local:local/many` in instance"))?;
        let foo_function = instance
            .get_export(&mut store, Some(&baz_instance), "foo")
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;
        let foo_function = instance
            .get_func(&mut store, foo_function)
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;

        let make = |input: &str| {
            let stuff = Val::Record(vec![
                (
                    "a".into(),
                    Val::List(e.a.iter().map(|v| Val::S32(*v)).collect()),
                ),
                ("b".into(), Val::Bool(e.b)),
                ("c".into(), Val::U64(e.c)),
            ]);
            vec![
                Val::String(input.to_owned()),
                Val::U32(b),
                Val::List(c.iter().map(|v| Val::U8(*v)).collect()),
                Val::Tuple(vec![Val::U64(d.0), Val::U64(d.1)]),
                stuff.clone(),
                Val::Option(Some(Box::new(stuff))),
                Val::Result(Err(None)),
            ]
        };

        // Start three concurrent calls and then join them all:
        let mut promises = PromisesUnordered::new();
        for (input, output) in inputs_and_outputs {
            let output = (*output).to_owned();
            promises.push(
                foo_function
                    .call_concurrent(&mut store, make(input))
                    .await?
                    .map(move |v| (v, output)),
            );
        }

        while let Some((actual, expected)) = promises.next(&mut store).await? {
            let Some(Val::Tuple(actual)) = actual.into_iter().next() else {
                unreachable!()
            };
            assert_eq!(make(&expected), actual);
        }
    }

    Ok(())
}

pub async fn test_round_trip_many_uncomposed(component: &[u8]) -> Result<()> {
    test_round_trip_many(
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

pub async fn test_round_trip_many_composed(a: &[u8], b: &[u8]) -> Result<()> {
    test_round_trip_many(
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
