use core::future::Future;

use std::collections::HashMap;

use tokio::sync::mpsc;
use wasmtime::component::{
    AbortOnDropHandle, Accessor, AccessorTask, FutureWriter, Linker, Lower, ResourceTable,
    StreamWriter,
};

use crate::p3::bindings::LinkOptions;

pub mod bindings;
pub mod cli;
pub mod clocks;
pub mod filesystem;
pub mod random;
pub mod sockets;

/// Add all WASI interfaces from this module into the `linker` provided.
///
/// This function will add the `async` variant of all interfaces into the
/// [`Linker`] provided. By `async` this means that this function is only
/// compatible with [`Config::async_support(true)`][async]. For embeddings with
/// async support disabled see [`add_to_linker_sync`] instead.
///
/// This function will add all interfaces implemented by this crate to the
/// [`Linker`], which corresponds to the `wasi:cli/imports` world supported by
/// this crate.
///
/// [async]: wasmtime::Config::async_support
///
/// # Example
///
/// ```
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker};
/// use wasmtime_wasi::p3::cli::{WasiCliCtx, WasiCliView};
/// use wasmtime_wasi::p3::clocks::{WasiClocksCtx, WasiClocksView};
/// use wasmtime_wasi::p3::filesystem::{WasiFilesystemCtx, WasiFilesystemView};
/// use wasmtime_wasi::p3::random::{WasiRandomCtx, WasiRandomView};
/// use wasmtime_wasi::p3::sockets::{WasiSocketsCtx, WasiSocketsView};
/// use wasmtime_wasi::p3::ResourceView;
///
/// fn main() -> Result<()> {
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p3::add_to_linker(&mut linker)?;
///     // ... add any further functionality to `linker` if desired ...
///
///     let mut store = Store::new(
///         &engine,
///         MyState::default(),
///     );
///
///     // ... use `linker` to instantiate within `store` ...
///
///     Ok(())
/// }
///
/// #[derive(Default)]
/// struct MyState {
///     cli: WasiCliCtx,
///     clocks: WasiClocksCtx,
///     filesystem: WasiFilesystemCtx,
///     random: WasiRandomCtx,
///     sockets: WasiSocketsCtx,
///     table: ResourceTable,
/// }
///
/// impl ResourceView for MyState {
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
///
/// impl WasiCliView for MyState {
///     fn cli(&self) -> &WasiCliCtx { &self.cli }
/// }
///
/// impl WasiClocksView for MyState {
///     fn clocks(&self) -> &WasiClocksCtx { &self.clocks }
/// }
///
/// impl WasiFilesystemView for MyState {
///     fn filesystem(&self) -> &WasiFilesystemCtx { &self.filesystem }
/// }
///
/// impl WasiRandomView for MyState {
///     fn random(&mut self) -> &mut WasiRandomCtx { &mut self.random }
/// }
///
/// impl WasiSocketsView for MyState {
///     fn sockets(&self) -> &WasiSocketsCtx { &self.sockets }
/// }
/// ```
pub fn add_to_linker<T>(linker: &mut Linker<T>) -> wasmtime::Result<()>
where
    T: clocks::WasiClocksView
        + random::WasiRandomView
        + sockets::WasiSocketsView
        + filesystem::WasiFilesystemView
        + cli::WasiCliView
        + 'static,
{
    let options = LinkOptions::default();
    add_to_linker_with_options(linker, &options)
}

/// Similar to [`add_to_linker`], but with the ability to enable unstable features.
pub fn add_to_linker_with_options<T>(
    linker: &mut Linker<T>,
    options: &LinkOptions,
) -> anyhow::Result<()>
where
    T: clocks::WasiClocksView
        + random::WasiRandomView
        + sockets::WasiSocketsView
        + filesystem::WasiFilesystemView
        + cli::WasiCliView
        + 'static,
{
    clocks::add_to_linker(linker)?;
    random::add_to_linker(linker)?;
    sockets::add_to_linker(linker)?;
    filesystem::add_to_linker(linker)?;
    cli::add_to_linker_with_options(linker, &options.into())?;
    Ok(())
}

pub trait ResourceView {
    fn table(&mut self) -> &mut ResourceTable;
}

impl<T: ResourceView> ResourceView for &mut T {
    fn table(&mut self) -> &mut ResourceTable {
        (**self).table()
    }
}

pub struct AccessorTaskFn<F>(pub F);

impl<T, U, R, F, Fut> AccessorTask<T, U, R> for AccessorTaskFn<F>
where
    F: FnOnce(&mut Accessor<T, U>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + Sync,
{
    fn run(self, accessor: &mut Accessor<T, U>) -> impl Future<Output = R> + Send + Sync {
        self.0(accessor)
    }
}

pub struct IoTask<T, E> {
    pub data: StreamWriter<T>,
    pub result: FutureWriter<Result<(), E>>,
    pub rx: mpsc::Receiver<Result<Vec<T>, E>>,
}

impl<T, U, O, E> AccessorTask<T, U, wasmtime::Result<()>> for IoTask<O, E>
where
    O: Lower + Send + Sync + 'static,
    E: Lower + Send + Sync + 'static,
{
    async fn run(mut self, _: &mut Accessor<T, U>) -> wasmtime::Result<()> {
        let mut tx = self.data;
        let res = loop {
            match self.rx.recv().await {
                None => {
                    drop(tx);
                    break Ok(());
                }
                Some(Ok(buf)) => {
                    let fut = tx.write(buf);
                    let Some(tail) = fut.into_future().await else {
                        break Ok(());
                    };
                    tx = tail;
                }
                Some(Err(err)) => {
                    // TODO: Close the stream with an error context
                    drop(tx);
                    break Err(err.into());
                }
            }
        };
        self.result.write(res).into_future().await;
        Ok(())
    }
}

#[derive(Default)]
pub struct TaskTable {
    tasks: HashMap<u32, AbortOnDropHandle>,
    next_task_id: u32,
    free_task_ids: Vec<u32>,
}

impl TaskTable {
    pub fn push(&mut self, handle: AbortOnDropHandle) -> Option<u32> {
        let id = if let Some(id) = self.free_task_ids.pop() {
            id
        } else {
            let id = self.next_task_id;
            let next = self.next_task_id.checked_add(1)?;
            self.next_task_id = next;
            id
        };
        self.tasks.insert(id, handle);
        Some(id)
    }

    pub fn remove(&mut self, id: u32) -> Option<AbortOnDropHandle> {
        let handle = self.tasks.remove(&id)?;
        self.free_task_ids.push(id);
        Some(handle)
    }
}
