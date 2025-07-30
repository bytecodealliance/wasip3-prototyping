use crate::p3::bindings::LinkOptions;
use crate::p3::cli::WasiCliCtxView;
use crate::sockets::WasiSocketsCtxView;
use anyhow::{Result, anyhow, bail};
use core::future::Future;
use core::ops::{Deref, DerefMut};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use wasmtime::component::{
    AbortHandle, Access, Accessor, AccessorTask, FutureWriter, GuardedStreamWriter, HasData,
    Linker, Lower, ResourceTable, StreamWriter, VecBuffer,
};

pub mod bindings;
pub mod cli;
pub mod clocks;
mod ctx;
pub mod filesystem;
pub mod random;
pub mod sockets;
mod view;

pub use self::ctx::{WasiCtx, WasiCtxBuilder};
pub use self::view::{WasiCtxView, WasiView};

// Default buffer capacity to use for reads of byte-sized values.
const DEFAULT_BUFFER_CAPACITY: usize = 8192;

pub struct AbortOnDropHandle(pub AbortHandle);

impl Drop for AbortOnDropHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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
/// ```ignore(TODO fix after upstreaming is complete)
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime_wasi::p3::filesystem::{WasiFilesystemCtx, WasiFilesystemView};
/// use wasmtime::component::{Linker, ResourceTable};
/// use wasmtime_wasi::p3::{WasiCtx, WasiCtxView, WasiView};
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
///     filesystem: WasiFilesystemCtx,
///     ctx: WasiCtx,
///     table: ResourceTable,
/// }
///
///
/// impl WasiFilesystemView for MyState {
///     fn filesystem(&self) -> &WasiFilesystemCtx { &self.filesystem }
/// }
///
/// impl WasiView for MyState {
///     fn ctx(&mut self) -> WasiCtxView<'_> {
///         WasiCtxView{
///             ctx: &mut self.ctx,
///             table: &mut self.table,
///         }
///     }
/// }
/// ```
pub fn add_to_linker<T>(linker: &mut Linker<T>) -> wasmtime::Result<()>
where
    T: WasiView + filesystem::WasiFilesystemView + 'static,
{
    let options = LinkOptions::default();
    add_to_linker_with_options(linker, &options)
}

/// Similar to [`add_to_linker`], but with the ability to enable unstable features.
pub fn add_to_linker_with_options<T>(
    linker: &mut Linker<T>,
    options: &LinkOptions,
) -> wasmtime::Result<()>
where
    T: WasiView + filesystem::WasiFilesystemView + 'static,
{
    cli::add_to_linker_impl(linker, &options.into(), |x| {
        let WasiCtxView { ctx, table } = x.ctx();
        WasiCliCtxView {
            ctx: &mut ctx.cli,
            table,
        }
    })?;
    clocks::add_to_linker_impl(linker, |x| &mut x.ctx().ctx.clocks)?;
    random::add_to_linker_impl(linker, |x| &mut x.ctx().ctx.random)?;
    sockets::add_to_linker_impl(linker, |x| {
        let WasiCtxView { ctx, table } = x.ctx();
        WasiSocketsCtxView {
            ctx: &mut ctx.sockets,
            table,
        }
    })?;
    filesystem::add_to_linker(linker)?;
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

/// Helper trait to use `spawn_*` functions below that benefit from type
/// inference where there's a single closure parameter.
pub trait SpawnExt: Sized {
    type Data: 'static;
    type AccessorData: HasData;

    fn spawn(
        self,
        task: impl AccessorTask<Self::Data, Self::AccessorData, Result<()>>,
    ) -> AbortHandle;

    fn spawn_fn<F>(
        self,
        func: impl FnOnce(&Accessor<Self::Data, Self::AccessorData>) -> F + Send + 'static,
    ) -> AbortHandle
    where
        F: Future<Output = Result<()>> + Send,
    {
        struct AccessorTaskFn<F>(pub F);

        impl<T, U, R, F, Fut> AccessorTask<T, U, R> for AccessorTaskFn<F>
        where
            U: HasData,
            F: FnOnce(&Accessor<T, U>) -> Fut + Send + 'static,
            Fut: Future<Output = R> + Send,
        {
            fn run(self, accessor: &Accessor<T, U>) -> impl Future<Output = R> + Send {
                self.0(accessor)
            }
        }

        self.spawn(AccessorTaskFn(func))
    }

    fn spawn_fn_box(
        self,
        func: impl FnOnce(
            &Accessor<Self::Data, Self::AccessorData>,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>
        + Send
        + 'static,
    ) -> AbortHandle {
        struct AccessorTaskFnBox<F>(pub F);

        impl<T, U, R, F> AccessorTask<T, U, R> for AccessorTaskFnBox<F>
        where
            U: HasData,
            F: FnOnce(&Accessor<T, U>) -> Pin<Box<dyn Future<Output = R> + Send + '_>>
                + Send
                + 'static,
        {
            async fn run(self, accessor: &Accessor<T, U>) -> R {
                self.0(accessor).await
            }
        }
        self.spawn(AccessorTaskFnBox(func))
    }
}

impl<T, D: HasData> SpawnExt for &mut Access<'_, T, D> {
    type Data = T;
    type AccessorData = D;

    fn spawn(self, task: impl AccessorTask<T, D, Result<()>>) -> AbortHandle {
        <Access<'_, T, D>>::spawn(self, task)
    }
}

impl<T, D: HasData> SpawnExt for &Accessor<T, D> {
    type Data = T;
    type AccessorData = D;

    fn spawn(self, task: impl AccessorTask<T, D, Result<()>>) -> AbortHandle {
        <Accessor<T, D>>::spawn(self, task)
    }
}

pub struct IoTask<T, E: 'static> {
    pub data: StreamWriter<T>,
    pub result: FutureWriter<Result<(), E>>,
    pub rx: mpsc::Receiver<Result<Vec<T>, E>>,
}

impl<T, U, O, E> AccessorTask<T, U, wasmtime::Result<()>> for IoTask<O, E>
where
    U: HasData,
    O: Lower + Send + Sync + 'static,
    E: Lower + Send + Sync + 'static,
{
    async fn run(mut self, store: &Accessor<T, U>) -> wasmtime::Result<()> {
        let mut tx = GuardedStreamWriter::new(store, self.data);
        let res = loop {
            match self.rx.recv().await {
                None => {
                    drop(tx);
                    break Ok(());
                }
                Some(Ok(buf)) => {
                    tx.write_all(VecBuffer::from(buf)).await;
                    if tx.is_closed() {
                        break Ok(());
                    }
                }
                Some(Err(err)) => {
                    // TODO: Close the stream with an error context
                    drop(tx);
                    break Err(err);
                }
            }
        };
        self.result.write(store, res).await;
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
