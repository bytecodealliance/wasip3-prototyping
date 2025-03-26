use std::iter;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::fs;
use wasmtime::component::{Component, Linker, PromisesUnordered, ResourceTable, Val};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::util::{annotate, compose, init_logger};
use component_async_tests::Ctx;

#[tokio::test]
pub async fn async_round_trip_many_stackless() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_many_stackful() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_many_synchronous() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
pub async fn async_round_trip_many_wait() -> Result<()> {
    test_round_trip_many_uncomposed(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?,
    )
    .await
}

#[tokio::test]
async fn async_round_trip_many_stackless_plus_stackless() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(stackless, stackless).await
}

#[tokio::test]
async fn async_round_trip_many_synchronous_plus_stackless() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, stackless).await
}

#[tokio::test]
async fn async_round_trip_many_stackless_plus_synchronous() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(stackless, synchronous).await
}

#[tokio::test]
async fn async_round_trip_many_synchronous_plus_synchronous() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, synchronous).await
}

#[tokio::test]
async fn async_round_trip_many_wait_plus_wait() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    test_round_trip_many_composed(wait, wait).await
}

#[tokio::test]
async fn async_round_trip_many_synchronous_plus_wait() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, wait).await
}

#[tokio::test]
async fn async_round_trip_many_wait_plus_synchronous() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(wait, synchronous).await
}

#[tokio::test]
async fn async_round_trip_many_stackless_plus_wait() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    test_round_trip_many_composed(stackless, wait).await
}

#[tokio::test]
async fn async_round_trip_many_wait_plus_stackless() -> Result<()> {
    let wait = &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_WAIT_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(wait, stackless).await
}

#[tokio::test]
async fn async_round_trip_many_stackful_plus_stackful() -> Result<()> {
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    test_round_trip_many_composed(stackful, stackful).await
}

#[tokio::test]
async fn async_round_trip_many_stackful_plus_stackless() -> Result<()> {
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    test_round_trip_many_composed(stackful, stackless).await
}

#[tokio::test]
async fn async_round_trip_many_stackless_plus_stackful() -> Result<()> {
    let stackless =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKLESS_COMPONENT).await?;
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    test_round_trip_many_composed(stackless, stackful).await
}

#[tokio::test]
async fn async_round_trip_many_synchronous_plus_stackful() -> Result<()> {
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    test_round_trip_many_composed(synchronous, stackful).await
}

#[tokio::test]
async fn async_round_trip_many_stackful_plus_synchronous() -> Result<()> {
    let stackful =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_STACKFUL_COMPONENT).await?;
    let synchronous =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_MANY_SYNCHRONOUS_COMPONENT).await?;
    test_round_trip_many_composed(stackful, synchronous).await
}

async fn test_round_trip_many_uncomposed(component: &[u8]) -> Result<()> {
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

async fn test_round_trip_many(component: &[u8], inputs_and_outputs: &[(&str, &str)]) -> Result<()> {
    use component_async_tests::round_trip_many::bindings::exports::local::local::many;

    init_logger();

    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.wasm_component_model_async_builtins(true);
    config.wasm_component_model_async_stackful(true);
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
        component_async_tests::round_trip_many::bindings::local::local::many::add_to_linker_get_host(
            &mut linker,
            annotate(|ctx| ctx),
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

        // Now do it again using `TypedFunc::call_async`-based bindings:
        let e = component_async_tests::round_trip_many::non_concurrent_export_bindings::exports::local::local::many::Stuff {
        a: vec![42i32; 42],
        b: true,
        c: 424242,
    };
        let f = Some(e.clone());
        let g = Err(());

        let round_trip_many = component_async_tests::round_trip_many::non_concurrent_export_bindings::RoundTripMany::instantiate_async(
            &mut store, &component, &linker,
        )
        .await?;

        for (input, expected) in inputs_and_outputs {
            assert_eq!(
                (
                    (*expected).to_owned(),
                    b,
                    c.clone(),
                    d,
                    e.clone(),
                    f.clone(),
                    g.clone()
                ),
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
                        g.clone()
                    )
                    .await?
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
            .func_new_concurrent("foo", |_, params| {
                Box::pin(async move {
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
                })
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

        // Now do it again using `Func::call_async`:
        for (input, expected) in inputs_and_outputs {
            let mut results = [Val::Bool(false)];
            foo_function
                .call_async(&mut store, &make(input), &mut results)
                .await?;
            let Val::Tuple(actual) = &results[0] else {
                unreachable!()
            };
            assert_eq!(&make(expected), actual);
        }
    }

    Ok(())
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
