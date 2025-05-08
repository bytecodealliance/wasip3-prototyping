use crate::p3::bindings::LinkOptions;
use anyhow::{anyhow, bail};
use core::future::Future;
use core::ops::{Deref, DerefMut};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use wasmtime::component::{
    AbortOnDropHandle, Accessor, AccessorTask, FutureWriter, HasData, Linker, Lower, ResourceTable,
    StreamWriter, VecBuffer,
};

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
///     fn cli(&mut self) -> &WasiCliCtx { &self.cli }
/// }
///
/// impl WasiClocksView for MyState {
///     fn clocks(&mut self) -> &WasiClocksCtx { &self.clocks }
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
    U: HasData,
    F: FnOnce(&mut Accessor<T, U>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + Send,
{
    fn run(self, accessor: &mut Accessor<T, U>) -> impl Future<Output = R> + Send {
        self.0(accessor)
    }
}

pub struct IoTask<T, E> {
    pub data: StreamWriter<VecBuffer<T>>,
    pub result: FutureWriter<Result<(), E>>,
    pub rx: mpsc::Receiver<Result<Vec<T>, E>>,
}

impl<T, U, O, E> AccessorTask<T, U, wasmtime::Result<()>> for IoTask<O, E>
where
    U: HasData,
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
                    let fut = tx.write_all(buf.into());
                    let (Some(tail), _) = fut.await else {
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
        self.result.write(res).await;
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

#[derive(Debug)]
pub enum WithChildren<T> {
    Parent(Arc<std::sync::RwLock<T>>),
    Child(Arc<std::sync::RwLock<T>>),
}

impl<T: Default> Default for WithChildren<T> {
    fn default() -> Self {
        Self::Parent(Arc::default())
    }
}

impl<T> WithChildren<T> {
    pub fn new(v: T) -> Self {
        Self::Parent(Arc::new(std::sync::RwLock::new(v)))
    }

    fn as_arc(&self) -> &Arc<std::sync::RwLock<T>> {
        match self {
            Self::Parent(v) | Self::Child(v) => v,
        }
    }

    fn into_arc(self) -> Arc<std::sync::RwLock<T>> {
        match self {
            Self::Parent(v) | Self::Child(v) => v,
        }
    }

    /// Returns a new child referencing the same value as `self`.
    pub fn child(&self) -> Self {
        Self::Child(Arc::clone(self.as_arc()))
    }

    /// Clone `T` and return the clone as a parent reference.
    /// Fails if the inner lock is poisoned.
    pub fn clone(&self) -> wasmtime::Result<Self>
    where
        T: Clone,
    {
        if let Ok(v) = self.as_arc().read() {
            Ok(Self::Parent(Arc::new(std::sync::RwLock::new(v.clone()))))
        } else {
            bail!("lock poisoned")
        }
    }

    /// If this is the only reference to `T` then unwrap it.
    /// Otherwise, clone `T` and return the clone.
    /// Fails if the inner lock is poisoned.
    pub fn unwrap_or_clone(self) -> wasmtime::Result<T>
    where
        T: Clone,
    {
        match Arc::try_unwrap(self.into_arc()) {
            Ok(v) => v.into_inner().map_err(|_| anyhow!("lock poisoned")),
            Err(v) => {
                if let Ok(v) = v.read() {
                    Ok(v.clone())
                } else {
                    bail!("lock poisoned")
                }
            }
        }
    }

    pub fn get(&self) -> wasmtime::Result<impl Deref<Target = T> + '_> {
        self.as_arc().read().map_err(|_| anyhow!("lock poisoned"))
    }

    pub fn get_mut(&mut self) -> wasmtime::Result<Option<impl DerefMut<Target = T> + '_>> {
        match self {
            Self::Parent(v) => {
                if let Ok(v) = v.write() {
                    Ok(Some(v))
                } else {
                    bail!("lock poisoned")
                }
            }
            Self::Child(..) => Ok(None),
        }
    }
}
