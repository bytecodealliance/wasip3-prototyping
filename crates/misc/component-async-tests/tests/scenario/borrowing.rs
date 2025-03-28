use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use tokio::fs;
use wasmtime::component::{Component, Linker, PromisesUnordered, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::util::{annotate, compose, init_logger};

#[tokio::test]
pub async fn async_borrowing_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    test_run_bool(&compose(caller, callee).await?, false).await
}

#[tokio::test]
async fn async_borrowing_caller_misbehave() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    let error = format!(
        "{:?}",
        test_run_bool(&compose(caller, callee).await?, true)
            .await
            .unwrap_err()
    );
    assert!(error.contains("unknown handle index"), "{error}");
    Ok(())
}

#[tokio::test]
async fn async_borrowing_callee_misbehave() -> Result<()> {
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    let error = format!("{:?}", test_run_bool(callee, true).await.unwrap_err());
    assert!(error.contains("unknown handle index"), "{error}");
    Ok(())
}

#[tokio::test]
pub async fn async_borrowing_callee() -> Result<()> {
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    test_run_bool(callee, false).await
}

pub async fn test_run_bool(component: &[u8], v: bool) -> Result<()> {
    init_logger();

    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);
    config.epoch_interruption(true);

    let engine = Engine::new(&config)?;

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    component_async_tests::borrowing_host::bindings::local::local::borrowing_types::add_to_linker_get_host(
        &mut linker,
        annotate(|ctx| ctx),
    )?;

    let mut store = Store::new(
        &engine,
        component_async_tests::Ctx {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::default(),
            continue_: false,
            wakers: Arc::new(Mutex::new(None)),
        },
    );
    store.set_epoch_deadline(1);

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        engine.increment_epoch();
    });

    let borrowing_host =
        component_async_tests::borrowing_host::bindings::BorrowingHost::instantiate_async(
            &mut store, &component, &linker,
        )
        .await?;

    // Start three concurrent calls and then join them all:
    let mut promises = PromisesUnordered::new();
    for _ in 0..3 {
        promises.push(
            borrowing_host
                .local_local_run_bool()
                .call_run(&mut store, v)
                .await?,
        );
    }

    while let Some(()) = promises.next(&mut store).await? {
        // continue
    }

    Ok(())
}
