use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Wake, Waker};
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures::{
    channel::oneshot,
    stream::{FuturesUnordered, TryStreamExt},
    FutureExt,
};
use once_cell::sync::Lazy;
use tokio::fs;
use wasmtime::component::{
    Accessor, AccessorTask, Component, Instance, Linker, ResourceTable, Val,
};
use wasmtime::{Engine, Store};
use wasmtime_wasi::p2::WasiCtxBuilder;

use component_async_tests::Ctx;

pub use component_async_tests::util::{annotate, compose, config};

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
pub async fn async_round_trip_stackless_joined() -> Result<()> {
    let component =
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT).await?;

    tokio::join!(
        async { test_round_trip_uncomposed(component).await.unwrap() },
        async { test_round_trip_uncomposed(component).await.unwrap() },
    );

    Ok(())
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
    let engine = Engine::new(&config())?;

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

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        component_async_tests::round_trip::bindings::local::local::baz::add_to_linker_get_host(
            &mut linker,
            annotate(|ctx| ctx),
        )?;

        let mut store = make_store();

        let instance = linker.instantiate_async(&mut store, &component).await?;
        let round_trip =
            component_async_tests::round_trip::bindings::RoundTrip::new(&mut store, &instance)?;

        // Start concurrent calls and then join them all:
        let mut futures = FuturesUnordered::new();
        for (input, output) in inputs_and_outputs {
            let output = (*output).to_owned();
            futures.push(
                round_trip
                    .local_local_baz()
                    .call_foo(&mut store, (*input).to_owned())
                    .map(move |v| v.map(move |v| (v, output))),
            );
        }

        while let Some((actual, expected)) = instance.run(&mut store, futures.try_next()).await?? {
            assert_eq!(expected, actual);
        }

        // Now do it again using `Instance::run_with`:
        instance
            .run_with(&mut store, {
                let inputs_and_outputs = inputs_and_outputs
                    .iter()
                    .map(|(a, b)| (String::from(*a), String::from(*b)))
                    .collect::<Vec<_>>();

                move |accessor| {
                    Box::pin(async move {
                        let mut futures = FuturesUnordered::new();
                        accessor.with(|mut store| {
                            for (input, output) in &inputs_and_outputs {
                                let output = output.clone();
                                futures.push(
                                    round_trip
                                        .local_local_baz()
                                        .call_foo(&mut store, input.clone())
                                        .map(move |v| v.map(move |v| (v, output)))
                                        .boxed(),
                                );
                            }
                        });

                        while let Some((actual, expected)) = futures.try_next().await? {
                            assert_eq!(expected, actual);
                        }

                        Ok::<_, wasmtime::Error>(())
                    })
                }
            })
            .await??;

        // And again using `Instance::spawn`:
        struct Task {
            instance: Instance,
            inputs_and_outputs: Vec<(String, String)>,
            tx: oneshot::Sender<()>,
        }

        impl AccessorTask<Ctx, Ctx, Result<()>> for Task {
            async fn run(self, accessor: &mut Accessor<Ctx, Ctx>) -> Result<()> {
                let mut futures = FuturesUnordered::new();
                accessor.with(|mut store| {
                    let round_trip = component_async_tests::round_trip::bindings::RoundTrip::new(
                        &mut store,
                        &self.instance,
                    )?;

                    for (input, output) in &self.inputs_and_outputs {
                        let output = output.clone();
                        futures.push(
                            round_trip
                                .local_local_baz()
                                .call_foo(&mut store, input.clone())
                                .map(move |v| v.map(move |v| (v, output)))
                                .boxed(),
                        );
                    }

                    Ok::<_, wasmtime::Error>(())
                })?;

                while let Some((actual, expected)) = futures.try_next().await? {
                    assert_eq!(expected, actual);
                }

                _ = self.tx.send(());

                Ok(())
            }
        }

        let (tx, rx) = oneshot::channel();
        instance.spawn(
            &mut store,
            Task {
                instance,
                inputs_and_outputs: inputs_and_outputs
                    .iter()
                    .map(|(a, b)| (String::from(*a), String::from(*b)))
                    .collect::<Vec<_>>(),
                tx,
            },
        );

        instance.run(&mut store, rx).await??;

        // And again using `TypedFunc::call_async`-based bindings:
        let round_trip =
            component_async_tests::round_trip::non_concurrent_export_bindings::RoundTrip::new(
                &mut store, &instance,
            )?;

        for (input, expected) in inputs_and_outputs {
            assert_eq!(
                *expected,
                &round_trip
                    .local_local_baz()
                    .call_foo(&mut store, (*input).to_owned())
                    .await?
            );
        }
    }

    // Now do it again using the dynamic API (except for WASI, where we stick with the static API):
    {
        let mut linker = Linker::new(&engine);

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        linker
            .root()
            .instance("local:local/baz")?
            .func_new_concurrent("[async]foo", |_, params| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    let Some(Val::String(s)) = params.into_iter().next() else {
                        unreachable!()
                    };
                    Ok(vec![Val::String(format!(
                        "{s} - entered host - exited host"
                    ))])
                })
            })?;

        let mut store = make_store();

        let instance = linker.instantiate_async(&mut store, &component).await?;
        let baz_instance = instance
            .get_export_index(&mut store, None, "local:local/baz")
            .ok_or_else(|| anyhow!("can't find `local:local/baz` in instance"))?;
        let foo_function = instance
            .get_export_index(&mut store, Some(&baz_instance), "[async]foo")
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;
        let foo_function = instance
            .get_func(&mut store, foo_function)
            .ok_or_else(|| anyhow!("can't find `foo` in instance"))?;

        // Start three concurrent calls and then join them all:
        let mut futures = FuturesUnordered::new();
        for (input, output) in inputs_and_outputs {
            let output = (*output).to_owned();
            futures.push(
                foo_function
                    .call_concurrent(&mut store, vec![Val::String((*input).to_owned())])
                    .map(move |v| v.map(move |v| (v, output))),
            );
        }

        while let Some((actual, expected)) = instance.run(&mut store, futures.try_next()).await?? {
            let Some(Val::String(actual)) = actual.into_iter().next() else {
                unreachable!()
            };
            assert_eq!(expected, actual);
        }

        // Now do it again using `Func::call_async`:
        for (input, expected) in inputs_and_outputs {
            let mut results = [Val::Bool(false)];
            foo_function
                .call_async(
                    &mut store,
                    &[Val::String((*input).to_owned())],
                    &mut results,
                )
                .await?;
            let Val::String(actual) = &results[0] else {
                unreachable!()
            };
            assert_eq!(*expected, actual);
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

enum PanicKind {
    DirectAwait,
    WrongInstance,
    RecursiveRun,
}

#[tokio::test]
#[should_panic(
    expected = "may only be polled from the event loop of the instance from which they originated"
)]
pub async fn panic_on_direct_await() {
    _ = test_panic(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT)
            .await
            .unwrap(),
        PanicKind::DirectAwait,
    )
    .await;
}

#[tokio::test]
#[should_panic(
    expected = "may only be polled from the event loop of the instance from which they originated"
)]
pub async fn panic_on_wrong_instance() {
    _ = test_panic(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT)
            .await
            .unwrap(),
        PanicKind::WrongInstance,
    )
    .await;
}

#[tokio::test]
#[should_panic(expected = "Recursive `Instance::run{_with}` calls not supported")]
pub async fn panic_on_recursive_run() {
    _ = test_panic(
        &fs::read(test_programs_artifacts::ASYNC_ROUND_TRIP_STACKLESS_COMPONENT)
            .await
            .unwrap(),
        PanicKind::RecursiveRun,
    )
    .await;
}

async fn test_panic(component: &[u8], kind: PanicKind) -> Result<()> {
    let inputs_and_outputs = &[("a", "b"), ("c", "d")];

    let engine = Engine::new(&config())?;

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

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    component_async_tests::round_trip::bindings::local::local::baz::add_to_linker_get_host(
        &mut linker,
        annotate(|ctx| ctx),
    )?;

    let mut store = make_store();

    let instance = linker.instantiate_async(&mut store, &component).await?;
    let round_trip =
        component_async_tests::round_trip::bindings::RoundTrip::new(&mut store, &instance)?;

    // Start concurrent calls and then join them all:
    let mut futures = FuturesUnordered::new();
    for (input, output) in inputs_and_outputs {
        let output = (*output).to_owned();
        futures.push(
            round_trip
                .local_local_baz()
                .call_foo(&mut store, (*input).to_owned())
                .map(move |v| v.map(move |v| (v, output))),
        );
    }

    match kind {
        PanicKind::DirectAwait => {
            // This should panic because we're polling it outside the instance's event loop:
            _ = futures.try_next().await?;
        }
        PanicKind::WrongInstance => {
            let instance = linker.instantiate_async(&mut store, &component).await?;
            // This should panic because we're polling it in the wrong instance's event loop:
            _ = instance.run(&mut store, futures.try_next()).await??;
        }
        PanicKind::RecursiveRun => {
            instance
                .run_with(&mut store, move |accessor| {
                    Box::pin(async move {
                        accessor.with(|mut access| {
                            // This should panic because we're already being polled inside the event loop:
                            _ = pin!(access.instance().run(&mut access, futures.try_next()))
                                .poll(&mut Context::from_waker(&dummy_waker()));
                        })
                    })
                })
                .await?;
        }
    }

    unreachable!()
}

fn dummy_waker() -> Waker {
    struct DummyWaker;

    impl Wake for DummyWaker {
        fn wake(self: Arc<Self>) {}
    }

    static WAKER: Lazy<Arc<DummyWaker>> = Lazy::new(|| Arc::new(DummyWaker));

    WAKER.clone().into()
}
