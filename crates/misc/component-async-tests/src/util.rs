use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use anyhow::Result;
use futures::stream::{FuturesUnordered, TryStreamExt};
use tokio::fs;
use wasm_compose::composer::ComponentComposer;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::p2::WasiCtxBuilder;

use super::{Ctx, sleep};

pub fn init_logger() {
    static ONCE: Once = Once::new();
    ONCE.call_once(env_logger::init);
}

pub fn config() -> Config {
    init_logger();

    let mut config = Config::new();
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.wasm_component_model_async_builtins(true);
    config.wasm_component_model_async_stackful(true);
    config.wasm_component_model_error_context(true);
    config.async_support(true);
    config
}

/// Compose two components
///
/// a is the "root" component, and b is composed into it
#[allow(unused)]
pub async fn compose(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir()?;

    let a_file = dir.path().join("a.wasm");
    fs::write(&a_file, a).await?;

    let b_file = dir.path().join("b.wasm");
    fs::write(&b_file, b).await?;

    ComponentComposer::new(
        &a_file,
        &wasm_compose::config::Config {
            dir: dir.path().to_owned(),
            definitions: vec![b_file.to_owned()],
            ..Default::default()
        },
    )
    .compose()
}

#[allow(unused)]
pub async fn test_run(component: &[u8]) -> Result<()> {
    test_run_with_count(component, 3).await
}

pub async fn test_run_with_count(component: &[u8], count: usize) -> Result<()> {
    let mut config = config();
    config.epoch_interruption(true);

    let engine = Engine::new(&config)?;

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    super::yield_host::bindings::local::local::continue_::add_to_linker::<_, Ctx>(
        &mut linker,
        |ctx| ctx,
    )?;
    super::yield_host::bindings::local::local::ready::add_to_linker::<_, Ctx>(
        &mut linker,
        |ctx| ctx,
    )?;
    super::resource_stream::bindings::local::local::resource_stream::add_to_linker::<_, Ctx>(
        &mut linker,
        |ctx| ctx,
    )?;
    sleep::local::local::sleep::add_to_linker::<_, Ctx>(&mut linker, |ctx| ctx)?;

    let mut store = Store::new(
        &engine,
        Ctx {
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

    let instance = linker.instantiate_async(&mut store, &component).await?;
    let yield_host = super::yield_host::bindings::YieldHost::new(&mut store, &instance)?;

    // Start `count` concurrent calls and then join them all:
    let mut futures = FuturesUnordered::new();
    for _ in 0..count {
        futures.push(yield_host.local_local_run().call_run(&mut store));
    }

    while let Some(()) = instance.run(&mut store, futures.try_next()).await?? {
        // continue
    }

    Ok(())
}
