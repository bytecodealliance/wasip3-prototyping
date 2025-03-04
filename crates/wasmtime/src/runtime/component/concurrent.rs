use {
    crate::{
        component::func::{self, Func, Lower as _, LowerContext, Options},
        store::StoreInner,
        vm::{
            component::{
                CallContext, ComponentInstance, InstanceFlags, ResourceTables, WaitableState,
            },
            mpk::{self, ProtectionMask},
            AsyncWasmCallState, PreviousAsyncWasmCallState, SendSyncPtr, VMFuncRef,
            VMMemoryDefinition, VMStore, VMStoreRawPtr,
        },
        AsContext, AsContextMut, Engine, StoreContext, StoreContextMut, ValRaw,
    },
    anyhow::{anyhow, bail, Context as _, Result},
    futures::{
        channel::oneshot,
        future::{self, FutureExt},
        stream::{FuturesUnordered, StreamExt},
    },
    once_cell::sync::Lazy,
    ready_chunks::ReadyChunks,
    std::{
        any::Any,
        borrow::ToOwned,
        boxed::Box,
        cell::UnsafeCell,
        collections::{HashMap, HashSet, VecDeque},
        future::Future,
        marker::PhantomData,
        mem::{self, MaybeUninit},
        ops::{Deref, DerefMut, Range},
        pin::{pin, Pin},
        ptr::{self, NonNull},
        sync::{Arc, Mutex},
        task::{Context, Poll, Wake, Waker},
        vec::Vec,
    },
    table::{Table, TableId},
    wasmtime_environ::{
        component::{
            InterfaceType, RuntimeComponentInstanceIndex, StringEncoding,
            TypeComponentLocalErrorContextTableIndex, TypeFutureTableIndex, TypeStreamTableIndex,
            TypeTupleIndex, MAX_FLAT_PARAMS, MAX_FLAT_RESULTS,
        },
        fact,
    },
    wasmtime_fiber::{Fiber, Suspend},
};

pub use futures_and_streams::{
    future, stream, ErrorContext, FutureReader, FutureWriter, HostFuture, HostStream, StreamReader,
    StreamWriter,
};
use futures_and_streams::{FlatAbi, TableIndex, TransmitState};

mod futures_and_streams;
mod ready_chunks;
mod table;

// TODO: The handling of `task.yield` and `task.backpressure` was bolted on late in the implementation and is
// currently haphazard.  We need a refactor to manage yielding, backpressure, and event polling and delivery in a
// more unified and structured way.

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
#[repr(u32)]
enum Status {
    Starting,
    Started,
    Returned,
    Done,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
#[repr(u32)]
enum Event {
    _Starting,
    Started,
    Returned,
    Done,
    _Yielded,
    StreamRead,
    StreamWrite,
    FutureRead,
    FutureWrite,
}

const EXIT_FLAG_ASYNC_CALLEE: u32 = fact::EXIT_FLAG_ASYNC_CALLEE as u32;

/// Represents the result of a concurrent operation.
///
/// This is similar to a [`std::future::Future`] except that it represents an
/// operation which requires exclusive access to a store in order to make
/// progress -- without monopolizing that store for the lifetime of the
/// operation.
pub struct Promise<T>(Pin<Box<dyn Future<Output = T> + Send + Sync + 'static>>);

impl<T: Send + Sync + 'static> Promise<T> {
    /// Map the result of this `Promise` from one value to another.
    pub fn map<U>(self, fun: impl FnOnce(T) -> U + Send + Sync + 'static) -> Promise<U> {
        Promise(Box::pin(self.0.map(fun)))
    }

    /// Convert this `Promise` to a future which may be `await`ed for its
    /// result.
    ///
    /// The returned future will require exclusive use of the store until it
    /// completes.  If you need to await more than one `Promise` concurrently,
    /// use [`PromisesUnordered`].
    pub async fn get<U: Send>(self, mut store: impl AsContextMut<Data = U>) -> Result<T> {
        Ok(poll_until(store.as_context_mut(), self.0).await?.1)
    }

    /// Convert this `Promise` to a future which may be `await`ed for its
    /// result.
    ///
    /// Unlike [`Self::get`], this does _not_ take a store parameter, meaning
    /// the returned future will not make progress until and unless the event
    /// loop for the store it came from is polled.  Thus, this method should
    /// only be used from within host functions and not from top-level embedder
    /// code.
    pub fn into_future(self) -> Pin<Box<dyn Future<Output = T> + Send + Sync + 'static>> {
        self.0
    }

    /// Wrap the specified `Future` in a `Promise`.
    pub fn from(fut: impl Future<Output = T> + Send + Sync + 'static) -> Self {
        Self(Box::pin(fut))
    }
}

/// Poll the specified future until it yields a result _or_ there are no more
/// tasks to run in the `Store`.
///
/// This is a more concise form of `Promise::from(fut).get(&mut store)`, plus it
/// supports non-`'static` and/or non-`Sync` `Future`s.
pub async fn get<U: Send, V: Send + Sync + 'static>(
    mut store: impl AsContextMut<Data = U>,
    fut: impl Future<Output = V> + Send,
) -> Result<V> {
    Ok(poll_until(store.as_context_mut(), Box::pin(fut)).await?.1)
}

/// Represents a collection of zero or more concurrent operations.
///
/// Similar to [`futures::stream::FuturesUnordered`], this type supports
/// `await`ing more than one [`Promise`]s concurrently.
pub struct PromisesUnordered<T>(
    FuturesUnordered<Pin<Box<dyn Future<Output = T> + Send + Sync + 'static>>>,
);

impl<T: Send + Sync + 'static> PromisesUnordered<T> {
    /// Create a new `PromisesUnordered` with no entries.
    pub fn new() -> Self {
        Self(FuturesUnordered::new())
    }

    /// Add the specified [`Promise`] to this collection.
    pub fn push(&mut self, promise: Promise<T>) {
        self.0.push(promise.0)
    }

    /// Get the next result from this collection, if any.
    pub async fn next<U: Send>(
        &mut self,
        mut store: impl AsContextMut<Data = U>,
    ) -> Result<Option<T>> {
        Ok(poll_until(
            store.as_context_mut(),
            Box::pin(self.0.next()) as Pin<Box<dyn Future<Output = Option<T>> + Send + Sync + '_>>,
        )
        .await?
        .1)
    }
}

/// Provides access to either store data (via the `get` method) or the store
/// itself (via [`AsContext`]/[`AsContextMut`]).
///
/// See [`Accessor::with`] for details.
pub struct Access<'a, T, U>(&'a mut Accessor<T, U>);

impl<'a, T, U> Access<'a, T, U> {
    /// Get mutable access to the store data.
    pub fn get(&mut self) -> &mut U {
        unsafe { &mut *(self.0.get)().0.cast() }
    }

    /// Spawn a background task.
    ///
    /// See [`Accessor::spawn`] for details.
    pub fn spawn(&mut self, task: impl AccessorTask<T, U, Result<()>>) -> SpawnHandle
    where
        T: 'static,
    {
        self.0.spawn(task)
    }
}

impl<'a, T, U> Deref for Access<'a, T, U> {
    type Target = U;

    fn deref(&self) -> &U {
        unsafe { &mut *(self.0.get)().0.cast() }
    }
}

impl<'a, T, U> DerefMut for Access<'a, T, U> {
    fn deref_mut(&mut self) -> &mut U {
        self.get()
    }
}

impl<'a, T, U> AsContext for Access<'a, T, U> {
    type Data = T;

    fn as_context(&self) -> StoreContext<T> {
        unsafe { StoreContext(&*(self.0.get)().1.cast()) }
    }
}

impl<'a, T, U> AsContextMut for Access<'a, T, U> {
    fn as_context_mut(&mut self) -> StoreContextMut<T> {
        unsafe { StoreContextMut(&mut *(self.0.get)().1.cast()) }
    }
}

/// Provides scoped mutable access to store data in the context of a concurrent
/// host import function.
///
/// This allows multiple host import futures to execute concurrently and access
/// the store data between (but not across) `await` points.
pub struct Accessor<T, U> {
    get: Arc<dyn Fn() -> (*mut u8, *mut u8)>,
    spawn: fn(Spawned),
    _phantom: PhantomData<fn() -> (*mut U, *mut StoreInner<T>)>,
}

unsafe impl<T, U> Send for Accessor<T, U> {}
unsafe impl<T, U> Sync for Accessor<T, U> {}

impl<T, U> Accessor<T, U> {
    #[doc(hidden)]
    pub unsafe fn new(get: fn() -> (*mut u8, *mut u8), spawn: fn(Spawned)) -> Self {
        Self {
            get: Arc::new(get),
            spawn,
            _phantom: PhantomData,
        }
    }

    /// Run the specified closure, passing it mutable access to the store data.
    ///
    /// Note that the return value of the closure must be `'static`, meaning it
    /// cannot borrow from the store data.  If you need shared access to
    /// something in the store data, it must be cloned (using e.g. `Arc::clone`
    /// if appropriate).
    pub fn with<R: 'static>(&mut self, fun: impl FnOnce(Access<'_, T, U>) -> R) -> R {
        fun(Access(self))
    }

    /// Run the specified task using a `Accessor` of a different type via the
    /// provided mapping function.
    ///
    /// This can be useful for projecting an `Accessor<T, U>` to an `Accessor<T,
    /// V>`, where `V` is the type of e.g. a field that can be mutably borrowed
    /// from a `&mut U`.
    pub async fn forward<R, V>(
        &mut self,
        fun: impl Fn(&mut U) -> &mut V + Send + Sync + 'static,
        task: impl AccessorTask<T, V, R>,
    ) -> R {
        let get = self.get.clone();
        let mut accessor = Accessor {
            get: Arc::new(move || {
                let (host, store) = get();
                unsafe { ((fun(&mut *host.cast()) as *mut V).cast(), store) }
            }),
            spawn: self.spawn,
            _phantom: PhantomData,
        };
        task.run(&mut accessor).await
    }

    /// Spawn a background task which will receive an `&mut Accessor<T, U>` and
    /// run concurrently with any other tasks in progress for the current
    /// instance.
    ///
    /// This is particularly useful for host functions which return a `stream`
    /// or `future` such that the code to write to the write end of that
    /// `stream` or `future` must run after the function returns.
    pub fn spawn(&mut self, task: impl AccessorTask<T, U, Result<()>>) -> SpawnHandle
    where
        T: 'static,
    {
        let mut accessor = Self {
            get: self.get.clone(),
            spawn: self.spawn,
            _phantom: PhantomData,
        };
        let future = Arc::new(Mutex::new(SpawnedInner::Unpolled(unsafe {
            // This is to avoid a `U: 'static` bound.  Rationale: We don't
            // actually store a value of type `U` in the `Accessor` we're
            // `move`ing into the `async` block, and access to a `U` is brokered
            // via `Accessor::with` by way of a thread-local variable in
            // `wasmtime-wit-bindgen`-generated code.  Furthermore,
            // `AccessorTask` implementations are required to be `'static`, so
            // no lifetime issues there.  We have no way to explain any of that
            // to the compiler, though, so we resort to a transmute here.
            mem::transmute::<
                Pin<Box<dyn Future<Output = Result<()>> + Send + Sync>>,
                Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'static>>,
            >(Box::pin(async move { task.run(&mut accessor).await }))
        })));
        let handle = SpawnHandle(future.clone());
        (self.spawn)(future);
        handle
    }
}

type SpawnedFuture = Pin<Box<dyn Future<Output = Result<()>> + Send + Sync + 'static>>;

#[doc(hidden)]
pub enum SpawnedInner {
    Unpolled(SpawnedFuture),
    Polled { future: SpawnedFuture, waker: Waker },
    Aborted,
}

impl SpawnedInner {
    fn abort(&mut self) {
        match mem::replace(self, Self::Aborted) {
            Self::Unpolled(_) | Self::Aborted => {}
            Self::Polled { waker, .. } => waker.wake(),
        }
    }
}

#[doc(hidden)]
pub type Spawned = Arc<Mutex<SpawnedInner>>;

/// Handle to a spawned task which may be used to abort it.
pub struct SpawnHandle(Spawned);

impl SpawnHandle {
    /// Abort the task.
    ///
    /// This will panic if called from within the spawned task itself.
    pub fn abort(&self) {
        self.0.try_lock().unwrap().abort()
    }

    /// Return an [`AbortOnDropHandle`] which, when dropped, will abort the
    /// task.
    ///
    /// The returned instance will panic if dropped from within the spawned task
    /// itself.
    pub fn abort_handle(&self) -> AbortOnDropHandle {
        AbortOnDropHandle(self.0.clone())
    }
}

/// Handle to a spawned task which will abort the task when dropped.
pub struct AbortOnDropHandle(Spawned);

impl Drop for AbortOnDropHandle {
    fn drop(&mut self) {
        self.0.try_lock().unwrap().abort()
    }
}

/// Represents a task which may be provided to `Accessor::spawn` or
/// `Accessor::forward`.
// TODO: Replace this with `std::ops::AsyncFnOnce` when that becomes a viable
// option.
//
// `AsyncFnOnce` is still nightly-only in latest stable Rust version as of this
// writing (1.84.1), and even with 1.85.0-beta it's not possible to specify
// e.g. `Send` and `Sync` bounds on the `Future` type returned by an
// `AsyncFnOnce`.  Also, using `F: Future<Output = Result<()>> + Send + Sync,
// FN: FnOnce(&mut Accessor<T>) -> F + Send + Sync + 'static` fails with a type
// mismatch error when we try to pass it an async closure (e.g. `async move |_|
// { ... }`).  So this seems to be the best we can do for the time being.
pub trait AccessorTask<T, U, R>: Send + Sync + 'static {
    /// Run the task.
    fn run(self, accessor: &mut Accessor<T, U>) -> impl Future<Output = R> + Send + Sync;
}

/// Trait representing component model ABI async intrinsics and fused adapter
/// helper functions.
pub unsafe trait VMComponentAsyncStore {
    /// The `task.backpressure` intrinsic.
    fn task_backpressure(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        enabled: u32,
    ) -> Result<()>;

    /// The `task.return` intrinsic.
    fn task_return(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeTupleIndex,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()>;

    /// The `task.wait` intrinsic.
    fn task_wait(
        &mut self,
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        memory: *mut VMMemoryDefinition,
        payload: u32,
    ) -> Result<u32>;

    /// The `task.poll` intrinsic.
    fn task_poll(
        &mut self,
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        memory: *mut VMMemoryDefinition,
        payload: u32,
    ) -> Result<u32>;

    /// The `task.yield` intrinsic.
    fn task_yield(&mut self, instance: &mut ComponentInstance, async_: bool) -> Result<()>;

    /// The `subtask.drop` intrinsic.
    fn subtask_drop(
        &mut self,
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        task_id: u32,
    ) -> Result<()>;

    /// A helper function for fused adapter modules involving calls where the
    /// caller is sync-lowered but the callee is async-lifted.
    fn sync_enter(
        &mut self,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        result_count: u32,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()>;

    /// A helper function for fused adapter modules involving calls where the
    /// caller is sync-lowered but the callee is async-lifted.
    fn sync_exit(
        &mut self,
        instance: &mut ComponentInstance,
        callback: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        callee: *mut VMFuncRef,
        callee_instance: RuntimeComponentInstanceIndex,
        param_count: u32,
        storage: *mut MaybeUninit<ValRaw>,
        storage_len: usize,
    ) -> Result<()>;

    /// A helper function for fused adapter modules involving calls where the
    /// caller is async-lowered.
    fn async_enter(
        &mut self,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        params: u32,
        results: u32,
    ) -> Result<()>;

    /// A helper function for fused adapter modules involving calls where the
    /// caller is async-lowered.
    fn async_exit(
        &mut self,
        instance: &mut ComponentInstance,
        callback: *mut VMFuncRef,
        post_return: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        callee: *mut VMFuncRef,
        callee_instance: RuntimeComponentInstanceIndex,
        param_count: u32,
        result_count: u32,
        flags: u32,
    ) -> Result<u32>;

    /// The `future.new` intrinsic.
    fn future_new(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
    ) -> Result<u32>;

    /// The `future.write` intrinsic.
    fn future_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeFutureTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32>;

    /// The `future.read` intrinsic.
    fn future_read(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeFutureTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32>;

    /// The `future.cancel-write` intrinsic.
    fn future_cancel_write(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        async_: bool,
        writer: u32,
    ) -> Result<u32>;

    /// The `future.cancel-read` intrinsic.
    fn future_cancel_read(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        async_: bool,
        reader: u32,
    ) -> Result<u32>;

    /// The `future.close-writable` intrinsic.
    fn future_close_writable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        error: u32,
    ) -> Result<()>;

    /// The `future.close-readable` intrinsic.
    fn future_close_readable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        reader: u32,
    ) -> Result<()>;

    /// The `stream.new` intrinsic.
    fn stream_new(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
    ) -> Result<u32>;

    /// The `stream.write` intrinsic.
    fn stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeStreamTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32>;

    /// The `stream.read` intrinsic.
    fn stream_read(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32>;

    /// The `stream.cancel-write` intrinsic.
    fn stream_cancel_write(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        async_: bool,
        writer: u32,
    ) -> Result<u32>;

    /// The `stream.cancel-read` intrinsic.
    fn stream_cancel_read(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        async_: bool,
        reader: u32,
    ) -> Result<u32>;

    /// The `stream.close-writable` intrinsic.
    fn stream_close_writable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        error: u32,
    ) -> Result<()>;

    /// The `stream.close-readable` intrinsic.
    fn stream_close_readable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        reader: u32,
    ) -> Result<()>;

    /// The "fast-path" implementation of the `stream.write` intrinsic for
    /// "flat" (i.e. memcpy-able) payloads.
    fn flat_stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        ty: TypeStreamTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32>;

    /// The "fast-path" implementation of the `stream.read` intrinsic for "flat"
    /// (i.e. memcpy-able) payloads.
    fn flat_stream_read(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32>;

    /// The `error-context.new` intrinsic.
    fn error_context_new(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        debug_msg_address: u32,
        debug_msg_len: u32,
    ) -> Result<u32>;

    /// The `error-context.debug-message` intrinsic.
    fn error_context_debug_message(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        err_ctx_handle: u32,
        debug_msg_address: u32,
    ) -> Result<()>;

    /// The `error-context.drop` intrinsic.
    fn error_context_drop(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeComponentLocalErrorContextTableIndex,
        err_ctx_handle: u32,
    ) -> Result<()>;
}

unsafe impl<T> VMComponentAsyncStore for StoreInner<T> {
    fn task_backpressure(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        enabled: u32,
    ) -> Result<()> {
        let mut cx = StoreContextMut(self);
        let entry = cx
            .concurrent_state()
            .instance_states
            .entry(caller_instance)
            .or_default();
        let old = entry.backpressure;
        let new = enabled != 0;
        entry.backpressure = new;

        if old && !new && !entry.task_queue.is_empty() {
            cx.concurrent_state().unblocked.insert(caller_instance);
        }

        Ok(())
    }

    fn task_return(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeTupleIndex,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()> {
        let storage = unsafe { std::slice::from_raw_parts(storage, storage_len) };
        let mut cx = StoreContextMut(self);
        let guest_task = cx.concurrent_state().guest_task.unwrap();
        let (lift, lift_ty) = cx
            .concurrent_state()
            .table
            .get_mut(guest_task)?
            .lift_result
            .take()
            .ok_or_else(|| anyhow!("`task.return` called more than once"))?;

        if ty != lift_ty {
            bail!("invalid `task.return` signature for current task");
        }

        assert!(cx
            .concurrent_state()
            .table
            .get(guest_task)?
            .result
            .is_none());

        log::trace!("task.return for {}", guest_task.rep());

        let (result, mut cx) = {
            let cx = cx.0.traitobj().as_ptr();
            let result = lift(cx, storage)?;
            let cx = unsafe { StoreContextMut::<T>(&mut *cx.cast()) };
            (result, cx)
        };

        let (calls, host_table, _) = cx.0.component_resource_state();
        ResourceTables {
            calls,
            host_table: Some(host_table),
            tables: Some((*instance).component_resource_tables()),
        }
        .exit_call()?;

        if let Caller::Host(tx) = &mut cx.concurrent_state().table.get_mut(guest_task)?.caller {
            _ = tx.take().unwrap().send(result);
        } else {
            cx.concurrent_state().table.get_mut(guest_task)?.result = Some(result);
        }

        Ok(())
    }

    fn task_wait(
        &mut self,
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        memory: *mut VMMemoryDefinition,
        payload: u32,
    ) -> Result<u32> {
        task_check(
            StoreContextMut(self),
            instance,
            async_,
            TaskCheck::Wait(memory, payload, caller_instance),
        )
    }

    fn task_poll(
        &mut self,
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        memory: *mut VMMemoryDefinition,
        payload: u32,
    ) -> Result<u32> {
        task_check(
            StoreContextMut(self),
            instance,
            async_,
            TaskCheck::Poll(memory, payload, caller_instance),
        )
    }

    fn task_yield(&mut self, instance: &mut ComponentInstance, async_: bool) -> Result<()> {
        task_check(StoreContextMut(self), instance, async_, TaskCheck::Yield).map(drop)
    }

    fn subtask_drop(
        &mut self,
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        task_id: u32,
    ) -> Result<()> {
        let mut cx = StoreContextMut(self);
        let (rep, WaitableState::Task) =
            instance.component_waitable_tables()[caller_instance].remove_by_index(task_id)?
        else {
            bail!("invalid task handle: {task_id}");
        };
        log::trace!("subtask_drop delete {rep}");
        cx.concurrent_state().yielding.remove(&rep);
        let table = &mut cx.concurrent_state().table;
        let task = table.delete_any(rep)?;
        let expected_caller_instance = match task.downcast::<HostTask>() {
            Ok(task) => task.caller_instance,
            Err(task) => match task.downcast::<GuestTask>() {
                Ok(task) => {
                    if let Caller::Guest { instance, .. } = task.caller {
                        instance
                    } else {
                        unreachable!()
                    }
                }
                Err(_) => unreachable!(),
            },
        };
        assert_eq!(expected_caller_instance, caller_instance);
        Ok(())
    }

    fn sync_enter(
        &mut self,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        result_count: u32,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()> {
        enter_call(
            StoreContextMut(self),
            start,
            return_,
            caller_instance,
            task_return_type,
            CallerInfo::Sync {
                params: unsafe { std::slice::from_raw_parts(storage, storage_len) }.to_vec(),
                result_count,
            },
        )
    }

    fn sync_exit(
        &mut self,
        instance: &mut ComponentInstance,
        callback: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        callee: *mut VMFuncRef,
        callee_instance: RuntimeComponentInstanceIndex,
        param_count: u32,
        storage: *mut MaybeUninit<ValRaw>,
        storage_len: usize,
    ) -> Result<()> {
        exit_call(
            StoreContextMut(self),
            instance,
            callback,
            ptr::null_mut(),
            caller_instance,
            callee,
            callee_instance,
            param_count,
            1,
            EXIT_FLAG_ASYNC_CALLEE,
            Some(unsafe { std::slice::from_raw_parts_mut(storage, storage_len) }),
        )
        .map(drop)
    }

    fn async_enter(
        &mut self,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        params: u32,
        results: u32,
    ) -> Result<()> {
        enter_call(
            StoreContextMut(self),
            start,
            return_,
            caller_instance,
            task_return_type,
            CallerInfo::Async { params, results },
        )
    }

    fn async_exit(
        &mut self,
        instance: &mut ComponentInstance,
        callback: *mut VMFuncRef,
        post_return: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        callee: *mut VMFuncRef,
        callee_instance: RuntimeComponentInstanceIndex,
        param_count: u32,
        result_count: u32,
        flags: u32,
    ) -> Result<u32> {
        exit_call(
            StoreContextMut(self),
            instance,
            callback,
            post_return,
            caller_instance,
            callee,
            callee_instance,
            param_count,
            result_count,
            flags,
            None,
        )
    }

    fn future_new(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
    ) -> Result<u32> {
        futures_and_streams::guest_new(StoreContextMut(self), instance, TableIndex::Future(ty))
    }

    fn future_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeFutureTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_write(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            string_encoding,
            TableIndex::Future(ty),
            None,
            future,
            address,
            1,
        )
    }

    fn future_read(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeFutureTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_read(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            string_encoding,
            TableIndex::Future(ty),
            err_ctx_ty,
            None,
            future,
            address,
            1,
        )
    }

    fn future_cancel_write(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        async_: bool,
        writer: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_cancel_write(
            StoreContextMut(self),
            instance,
            TableIndex::Future(ty),
            writer,
            async_,
        )
    }

    fn future_cancel_read(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        async_: bool,
        reader: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_cancel_read(
            StoreContextMut(self),
            instance,
            TableIndex::Future(ty),
            reader,
            async_,
        )
    }

    fn future_close_writable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        error: u32,
    ) -> Result<()> {
        futures_and_streams::guest_close_writable(
            StoreContextMut(self),
            instance,
            TableIndex::Future(ty),
            err_ctx_ty,
            writer,
            error,
        )
    }

    fn future_close_readable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeFutureTableIndex,
        reader: u32,
    ) -> Result<()> {
        futures_and_streams::guest_close_readable(
            StoreContextMut(self),
            instance,
            TableIndex::Future(ty),
            reader,
        )
    }

    fn stream_new(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
    ) -> Result<u32> {
        futures_and_streams::guest_new(StoreContextMut(self), instance, TableIndex::Stream(ty))
    }

    fn stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeStreamTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_write(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            string_encoding,
            TableIndex::Stream(ty),
            None,
            stream,
            address,
            count,
        )
    }

    fn stream_read(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_read(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            string_encoding,
            TableIndex::Stream(ty),
            err_ctx_ty,
            None,
            stream,
            address,
            count,
        )
    }

    fn stream_cancel_write(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        async_: bool,
        writer: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_cancel_write(
            StoreContextMut(self),
            instance,
            TableIndex::Stream(ty),
            writer,
            async_,
        )
    }

    fn stream_cancel_read(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        async_: bool,
        reader: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_cancel_read(
            StoreContextMut(self),
            instance,
            TableIndex::Stream(ty),
            reader,
            async_,
        )
    }

    fn stream_close_writable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        error: u32,
    ) -> Result<()> {
        futures_and_streams::guest_close_writable(
            StoreContextMut(self),
            instance,
            TableIndex::Stream(ty),
            err_ctx_ty,
            writer,
            error,
        )
    }

    fn stream_close_readable(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeStreamTableIndex,
        reader: u32,
    ) -> Result<()> {
        futures_and_streams::guest_close_readable(
            StoreContextMut(self),
            instance,
            TableIndex::Stream(ty),
            reader,
        )
    }

    fn flat_stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        ty: TypeStreamTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_write(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            StringEncoding::Utf8 as u8,
            TableIndex::Stream(ty),
            Some(FlatAbi {
                size: payload_size,
                align: payload_align,
            }),
            stream,
            address,
            count,
        )
    }

    fn flat_stream_read(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        futures_and_streams::guest_read(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            StringEncoding::Utf8 as u8,
            TableIndex::Stream(ty),
            err_ctx_ty,
            Some(FlatAbi {
                size: payload_size,
                align: payload_align,
            }),
            stream,
            address,
            count,
        )
    }

    fn error_context_new(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        debug_msg_address: u32,
        debug_msg_len: u32,
    ) -> Result<u32> {
        futures_and_streams::error_context_new(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            string_encoding,
            ty,
            debug_msg_address,
            debug_msg_len,
        )
    }

    fn error_context_debug_message(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        err_ctx_handle: u32,
        debug_msg_address: u32,
    ) -> Result<()> {
        futures_and_streams::error_context_debug_message(
            StoreContextMut(self),
            instance,
            memory,
            realloc,
            string_encoding,
            ty,
            err_ctx_handle,
            debug_msg_address,
        )
    }

    fn error_context_drop(
        &mut self,
        instance: &mut ComponentInstance,
        ty: TypeComponentLocalErrorContextTableIndex,
        err_ctx_handle: u32,
    ) -> Result<()> {
        futures_and_streams::error_context_drop(StoreContextMut(self), instance, ty, err_ctx_handle)
    }
}

struct HostTaskResult {
    event: Event,
    param: u32,
    caller: TableId<GuestTask>,
}

enum HostTaskOutput {
    Background(Result<()>),
    Call {
        waitable: u32,
        fun: Box<dyn FnOnce(*mut dyn VMStore) -> Result<HostTaskResult> + Send + Sync>,
    },
}

type HostTaskFuture = Pin<Box<dyn Future<Output = HostTaskOutput> + Send + Sync + 'static>>;

struct HostTask {
    caller_instance: RuntimeComponentInstanceIndex,
}

enum Deferred {
    None,
    Stackful {
        fiber: StoreFiber<'static>,
        async_: bool,
    },
    Stackless {
        call: Box<dyn FnOnce(*mut dyn VMStore) -> Result<u32> + Send + Sync + 'static>,
        instance: RuntimeComponentInstanceIndex,
        callback: SendSyncPtr<VMFuncRef>,
    },
}

impl Deferred {
    fn take_stackful(&mut self) -> Option<(StoreFiber<'static>, bool)> {
        if let Self::Stackful { .. } = self {
            let Self::Stackful { fiber, async_ } = mem::replace(self, Self::None) else {
                unreachable!()
            };
            Some((fiber, async_))
        } else {
            None
        }
    }
}

#[derive(Copy, Clone)]
struct Callback {
    function: SendSyncPtr<VMFuncRef>,
    context: u32,
    instance: RuntimeComponentInstanceIndex,
}

enum Caller {
    Host(Option<oneshot::Sender<LiftedResult>>),
    Guest {
        task: TableId<GuestTask>,
        instance: RuntimeComponentInstanceIndex,
    },
}

struct GuestTask {
    lower_params: Option<RawLower>,
    lift_result: Option<(RawLift, TypeTupleIndex)>,
    result: Option<LiftedResult>,
    callback: Option<Callback>,
    events: VecDeque<(Event, AnyTask, u32)>,
    caller: Caller,
    deferred: Deferred,
    should_yield: bool,
    call_context: Option<CallContext>,
    sync_result: Option<ValRaw>,
}

impl Default for GuestTask {
    fn default() -> Self {
        Self {
            lower_params: None,
            lift_result: None,
            result: None,
            callback: None,
            events: VecDeque::new(),
            caller: Caller::Host(None),
            deferred: Deferred::None,
            should_yield: false,
            call_context: Some(CallContext::default()),
            sync_result: None,
        }
    }
}

#[derive(Copy, Clone)]
enum AnyTask {
    Host(TableId<HostTask>),
    Guest(TableId<GuestTask>),
    Transmit(TableId<TransmitState>),
}

impl AnyTask {
    fn rep(&self) -> u32 {
        match self {
            Self::Host(task) => task.rep(),
            Self::Guest(task) => task.rep(),
            Self::Transmit(task) => task.rep(),
        }
    }

    fn delete_all_from<T>(&self, mut store: StoreContextMut<T>) -> Result<()> {
        match self {
            Self::Host(task) => {
                log::trace!("delete host task {}", task.rep());
                store.concurrent_state().table.delete(*task).map(drop)
            }
            Self::Guest(task) => {
                let finished = store
                    .concurrent_state()
                    .table
                    .get(*task)?
                    .events
                    .iter()
                    .filter_map(|(event, call, _)| (*event == Event::Done).then_some(*call))
                    .collect::<Vec<_>>();

                for call in finished {
                    log::trace!("will delete call {}", call.rep());
                    call.delete_all_from(store.as_context_mut())?;
                }

                log::trace!("delete guest task {}", task.rep());
                store.concurrent_state().yielding.remove(&task.rep());
                store.concurrent_state().table.delete(*task).map(drop)
            }
            Self::Transmit(task) => store.concurrent_state().table.delete(*task).map(drop),
        }?;

        Ok(())
    }
}

pub(crate) struct LiftLowerContext {
    pub(crate) pointer: *mut u8,
    pub(crate) dropper: fn(*mut u8),
}

unsafe impl Send for LiftLowerContext {}
unsafe impl Sync for LiftLowerContext {}

impl Drop for LiftLowerContext {
    fn drop(&mut self) {
        (self.dropper)(self.pointer);
    }
}

type RawLower =
    Box<dyn FnOnce(*mut dyn VMStore, &mut [MaybeUninit<ValRaw>]) -> Result<()> + Send + Sync>;

type LowerFn = fn(LiftLowerContext, *mut dyn VMStore, &mut [MaybeUninit<ValRaw>]) -> Result<()>;

type RawLift = Box<
    dyn FnOnce(*mut dyn VMStore, &[ValRaw]) -> Result<Box<dyn Any + Send + Sync>> + Send + Sync,
>;

type LiftFn =
    fn(LiftLowerContext, *mut dyn VMStore, &[ValRaw]) -> Result<Box<dyn Any + Send + Sync>>;

type LiftedResult = Box<dyn Any + Send + Sync>;

struct DummyResult;

struct Reset<T: Copy>(*mut T, T);

impl<T: Copy> Drop for Reset<T> {
    fn drop(&mut self) {
        unsafe {
            *self.0 = self.1;
        }
    }
}

#[derive(Clone, Copy)]
struct PollContext {
    future_context: *mut Context<'static>,
    guard_range_start: *mut u8,
    guard_range_end: *mut u8,
}

impl Default for PollContext {
    fn default() -> PollContext {
        PollContext {
            future_context: ptr::null_mut(),
            guard_range_start: ptr::null_mut(),
            guard_range_end: ptr::null_mut(),
        }
    }
}

struct AsyncState {
    current_suspend: UnsafeCell<
        *mut Suspend<
            (Option<*mut dyn VMStore>, Result<()>),
            Option<*mut dyn VMStore>,
            (Option<*mut dyn VMStore>, Result<()>),
        >,
    >,
    current_poll_cx: UnsafeCell<PollContext>,
}

unsafe impl Send for AsyncState {}
unsafe impl Sync for AsyncState {}

pub(crate) struct AsyncCx {
    current_suspend: *mut *mut wasmtime_fiber::Suspend<
        (Option<*mut dyn VMStore>, Result<()>),
        Option<*mut dyn VMStore>,
        (Option<*mut dyn VMStore>, Result<()>),
    >,
    current_stack_limit: *mut usize,
    current_poll_cx: *mut PollContext,
    track_pkey_context_switch: bool,
}

impl AsyncCx {
    pub(crate) fn new<T>(store: &mut StoreContextMut<T>) -> Self {
        Self::try_new(store).unwrap()
    }

    pub(crate) fn try_new<T>(store: &mut StoreContextMut<T>) -> Option<Self> {
        let current_poll_cx = store.concurrent_state().async_state.current_poll_cx.get();
        if unsafe { (*current_poll_cx).future_context.is_null() } {
            None
        } else {
            Some(Self {
                current_suspend: store.concurrent_state().async_state.current_suspend.get(),
                current_stack_limit: store.0.runtime_limits().stack_limit.get(),
                current_poll_cx,
                track_pkey_context_switch: store.has_pkey(),
            })
        }
    }

    unsafe fn poll<U>(&self, mut future: Pin<&mut (dyn Future<Output = U> + Send)>) -> Poll<U> {
        let poll_cx = *self.current_poll_cx;
        let _reset = Reset(self.current_poll_cx, poll_cx);
        *self.current_poll_cx = PollContext::default();
        assert!(!poll_cx.future_context.is_null());
        future.as_mut().poll(&mut *poll_cx.future_context)
    }

    pub(crate) unsafe fn block_on<'a, T, U>(
        &self,
        mut future: Pin<&mut (dyn Future<Output = U> + Send)>,
        mut store: Option<StoreContextMut<'a, T>>,
    ) -> Result<(U, Option<StoreContextMut<'a, T>>)> {
        loop {
            match self.poll(future.as_mut()) {
                Poll::Ready(v) => break Ok((v, store)),
                Poll::Pending => {}
            }

            store = self.suspend(store)?;
        }
    }

    unsafe fn suspend<'a, T>(
        &self,
        store: Option<StoreContextMut<'a, T>>,
    ) -> Result<Option<StoreContextMut<'a, T>>> {
        let previous_mask = if self.track_pkey_context_switch {
            let previous_mask = mpk::current_mask();
            mpk::allow(ProtectionMask::all());
            previous_mask
        } else {
            ProtectionMask::all()
        };
        let store = suspend_fiber(self.current_suspend, self.current_stack_limit, store);
        if self.track_pkey_context_switch {
            mpk::allow(previous_mask);
        }
        store
    }
}

#[derive(Default)]
struct InstanceState {
    backpressure: bool,
    in_sync_call: bool,
    task_queue: VecDeque<TableId<GuestTask>>,
}

pub struct ConcurrentState<T> {
    guest_task: Option<TableId<GuestTask>>,
    futures: ReadyChunks<FuturesUnordered<HostTaskFuture>>,
    table: Table,
    async_state: AsyncState,
    // TODO: this can and should be a `PrimaryMap`
    instance_states: HashMap<RuntimeComponentInstanceIndex, InstanceState>,
    yielding: HashSet<u32>,
    unblocked: HashSet<RuntimeComponentInstanceIndex>,
    component_instance: Option<SendSyncPtr<ComponentInstance>>,
    _phantom: PhantomData<T>,
}

impl<T> ConcurrentState<T> {
    pub(crate) fn async_guard_range(&self) -> Range<*mut u8> {
        let context = unsafe { *self.async_state.current_poll_cx.get() };
        context.guard_range_start..context.guard_range_end
    }

    pub(crate) fn spawn(&mut self, task: impl Future<Output = Result<()>> + Send + Sync + 'static) {
        self.futures.get_mut().push(Box::pin(
            async move { HostTaskOutput::Background(task.await) },
        ))
    }
}

impl<T> Default for ConcurrentState<T> {
    fn default() -> Self {
        Self {
            guest_task: None,
            table: Table::new(),
            futures: ReadyChunks::new(FuturesUnordered::new(), 1024),
            async_state: AsyncState {
                current_suspend: UnsafeCell::new(ptr::null_mut()),
                current_poll_cx: UnsafeCell::new(PollContext::default()),
            },
            instance_states: HashMap::new(),
            yielding: HashSet::new(),
            unblocked: HashSet::new(),
            component_instance: None,
            _phantom: PhantomData,
        }
    }
}

fn dummy_waker() -> Waker {
    struct DummyWaker;

    impl Wake for DummyWaker {
        fn wake(self: Arc<Self>) {}
    }

    static WAKER: Lazy<Arc<DummyWaker>> = Lazy::new(|| Arc::new(DummyWaker));

    WAKER.clone().into()
}

fn for_any_lower<
    F: FnOnce(*mut dyn VMStore, &mut [MaybeUninit<ValRaw>]) -> Result<()> + Send + Sync,
>(
    fun: F,
) -> F {
    fun
}

fn for_any_lift<
    F: FnOnce(*mut dyn VMStore, &[ValRaw]) -> Result<Box<dyn Any + Send + Sync>> + Send + Sync,
>(
    fun: F,
) -> F {
    fun
}

pub(crate) fn first_poll<T, R: Send + Sync + 'static>(
    instance: *mut ComponentInstance,
    mut store: StoreContextMut<T>,
    future: impl Future<Output = Result<R>> + Send + Sync + 'static,
    caller_instance: RuntimeComponentInstanceIndex,
    lower: impl FnOnce(StoreContextMut<T>, R) -> Result<()> + Send + Sync + 'static,
) -> Result<Option<u32>> {
    let caller = store.concurrent_state().guest_task.unwrap();
    let task = store
        .concurrent_state()
        .table
        .push_child(HostTask { caller_instance }, caller)?;

    // Here we wrap the future in a `future::poll_fn` to ensure that we restore
    // and save the `CallContext` for this task before and after polling it,
    // respectively.  This involves unsafe shenanigans in order to smuggle the
    // store pointer into the wrapping future, alas.

    fn maybe_push<T>(
        store: VMStoreRawPtr,
        call_context: &mut Option<CallContext>,
        task: TableId<HostTask>,
    ) {
        let store = unsafe { StoreContextMut::<T>(&mut *store.0.as_ptr().cast()) };
        if let Some(call_context) = call_context.take() {
            log::trace!("push call context for {}", task.rep());
            store.0.component_resource_state().0.push(call_context);
        }
    }

    fn pop<T>(
        store: VMStoreRawPtr,
        call_context: &mut Option<CallContext>,
        task: TableId<HostTask>,
    ) {
        log::trace!("pop call context for {}", task.rep());
        let store = unsafe { StoreContextMut::<T>(&mut *store.0.as_ptr().cast()) };
        *call_context = Some(store.0.component_resource_state().0.pop().unwrap());
    }

    let future = future::poll_fn({
        let mut future = Box::pin(future);
        let store = VMStoreRawPtr(store.0.traitobj());
        let mut call_context = None;
        move |cx| {
            maybe_push::<T>(store, &mut call_context, task);

            match future.as_mut().poll(cx) {
                Poll::Ready(output) => Poll::Ready(output),
                Poll::Pending => {
                    pop::<T>(store, &mut call_context, task);
                    Poll::Pending
                }
            }
        }
    });

    log::trace!("new child of {}: {}", caller.rep(), task.rep());
    let mut future = Box::pin(future.map(move |result| HostTaskOutput::Call {
        waitable: task.rep(),
        fun: Box::new(move |store: *mut dyn VMStore| {
            let store = unsafe { StoreContextMut(&mut *store.cast()) };
            lower(store, result?)?;
            Ok(HostTaskResult {
                event: Event::Done,
                param: 0u32,
                caller,
            })
        }),
    })) as HostTaskFuture;

    Ok(
        match future
            .as_mut()
            .poll(&mut Context::from_waker(&dummy_waker()))
        {
            Poll::Ready(output) => {
                let HostTaskOutput::Call { fun, .. } = output else {
                    unreachable!()
                };
                log::trace!("delete host task {} (already ready)", task.rep());
                store.concurrent_state().table.delete(task)?;
                fun(store.0.traitobj().as_ptr())?;
                None
            }
            Poll::Pending => {
                store.concurrent_state().futures.get_mut().push(future);
                Some(
                    unsafe { &mut *instance }.component_waitable_tables()[caller_instance]
                        .insert(task.rep(), WaitableState::Task)?,
                )
            }
        },
    )
}

pub(crate) fn poll_and_block<'a, T, R: Send + Sync + 'static>(
    mut store: StoreContextMut<'a, T>,
    future: impl Future<Output = Result<R>> + Send + Sync + 'static,
    caller_instance: RuntimeComponentInstanceIndex,
) -> Result<(R, StoreContextMut<'a, T>)> {
    let Some(caller) = store.concurrent_state().guest_task else {
        return match pin!(future).poll(&mut Context::from_waker(&dummy_waker())) {
            Poll::Ready(result) => Ok((result?, store)),
            Poll::Pending => {
                unreachable!()
            }
        };
    };
    let old_result = store
        .concurrent_state()
        .table
        .get_mut(caller)
        .with_context(|| format!("bad handle: {}", caller.rep()))?
        .result
        .take();
    let task = store
        .concurrent_state()
        .table
        .push_child(HostTask { caller_instance }, caller)?;
    log::trace!("new child of {}: {}", caller.rep(), task.rep());
    let mut future = Box::pin(future.map(move |result| HostTaskOutput::Call {
        waitable: task.rep(),
        fun: Box::new(move |store: *mut dyn VMStore| {
            let mut store = unsafe { StoreContextMut::<T>(&mut *store.cast()) };
            store.concurrent_state().table.get_mut(caller)?.result = Some(Box::new(result?) as _);
            Ok(HostTaskResult {
                event: Event::Done,
                param: 0u32,
                caller,
            })
        }),
    })) as HostTaskFuture;

    Ok(
        match unsafe { AsyncCx::new(&mut store).poll(future.as_mut()) } {
            Poll::Ready(output) => {
                let HostTaskOutput::Call { fun, .. } = output else {
                    unreachable!()
                };
                log::trace!("delete host task {} (already ready)", task.rep());
                store.concurrent_state().table.delete(task)?;
                let store = store.0.traitobj().as_ptr();
                fun(store)?;
                let mut store = unsafe { StoreContextMut(&mut *store.cast()) };
                let result = *mem::replace(
                    &mut store.concurrent_state().table.get_mut(caller)?.result,
                    old_result,
                )
                .unwrap()
                .downcast()
                .unwrap();
                (result, store)
            }
            Poll::Pending => {
                store.concurrent_state().futures.get_mut().push(future);

                store = poll_loop(store, |store| {
                    Ok(store
                        .concurrent_state()
                        .table
                        .get_mut(caller)?
                        .result
                        .is_none())
                })?;

                if let Some(result) = store
                    .concurrent_state()
                    .table
                    .get_mut(caller)?
                    .result
                    .take()
                {
                    store.concurrent_state().table.get_mut(caller)?.result = old_result;
                    (*result.downcast().unwrap(), store)
                } else {
                    return Err(anyhow!(crate::Trap::NoAsyncResult));
                }
            }
        },
    )
}

pub(crate) async fn on_fiber<'a, R: Send + Sync + 'static, T: Send>(
    mut store: StoreContextMut<'a, T>,
    instance: Option<RuntimeComponentInstanceIndex>,
    func: impl FnOnce(&mut StoreContextMut<T>) -> R + Send,
) -> Result<(R, StoreContextMut<'a, T>)> {
    let result = Arc::new(Mutex::new(None));
    let mut fiber = make_fiber(&mut store, instance, {
        let result = result.clone();
        move |mut store| {
            *result.lock().unwrap() = Some(func(&mut store));
            Ok(())
        }
    })?;

    let guard_range = fiber
        .fiber
        .as_ref()
        .unwrap()
        .stack()
        .guard_range()
        .map(|r| {
            (
                NonNull::new(r.start).map(SendSyncPtr::new),
                NonNull::new(r.end).map(SendSyncPtr::new),
            )
        })
        .unwrap_or((None, None));

    store = poll_fn(store, guard_range, move |_, mut store| {
        match resume_fiber(&mut fiber, store.take(), Ok(())) {
            Ok(Ok((store, result))) => Ok(result.map(|()| store)),
            Ok(Err(s)) => Err(s),
            Err(e) => Ok(Err(e)),
        }
    })
    .await?;

    let result = result.lock().unwrap().take().unwrap();
    Ok((result, store))
}

fn maybe_push_call_context<T>(
    store: &mut StoreContextMut<T>,
    guest_task: TableId<GuestTask>,
) -> Result<()> {
    let task = store.concurrent_state().table.get_mut(guest_task)?;
    if task.lift_result.is_some() {
        log::trace!("push call context for {}", guest_task.rep());
        let call_context = task.call_context.take().unwrap();
        store.0.component_resource_state().0.push(call_context);
    }
    Ok(())
}

fn maybe_pop_call_context<T>(
    store: &mut StoreContextMut<T>,
    guest_task: TableId<GuestTask>,
) -> Result<()> {
    if store
        .concurrent_state()
        .table
        .get_mut(guest_task)?
        .lift_result
        .is_some()
    {
        log::trace!("pop call context for {}", guest_task.rep());
        let call_context = Some(store.0.component_resource_state().0.pop().unwrap());
        store
            .concurrent_state()
            .table
            .get_mut(guest_task)?
            .call_context = call_context;
    }
    Ok(())
}

fn maybe_send_event<'a, T>(
    mut store: StoreContextMut<'a, T>,
    guest_task: TableId<GuestTask>,
    event: Event,
    call: AnyTask,
    result: u32,
) -> Result<StoreContextMut<'a, T>> {
    assert_ne!(guest_task.rep(), call.rep());
    if let Some(callback) = store.concurrent_state().table.get(guest_task)?.callback {
        let old_task = store.concurrent_state().guest_task.replace(guest_task);
        let Some((handle, _)) = unsafe {
            &mut *store
                .concurrent_state()
                .component_instance
                .unwrap()
                .as_ptr()
        }
        .component_waitable_tables()[callback.instance]
            .get_mut_by_rep(call.rep())
        else {
            // If there's no handle found for the subtask, that means either the
            // subtask was closed by the caller already or the caller called the
            // callee via a sync-lowered import.  Either way, we can skip this
            // event.
            if let Event::Done = event {
                // Since this is a `Done` event which will never be delivered to
                // the caller, we must handle deleting the subtask here.
                call.delete_all_from(store.as_context_mut())?;
            }
            return Ok(store);
        };
        log::trace!(
            "use callback to deliver event {event:?} to {} for {} (handle {handle}): {:?} {}",
            guest_task.rep(),
            call.rep(),
            callback.function,
            callback.context
        );

        maybe_push_call_context(&mut store, guest_task)?;

        let mut flags = unsafe {
            (*store
                .concurrent_state()
                .component_instance
                .unwrap()
                .as_ptr())
            .instance_flags(callback.instance)
        };

        let params = &mut [
            ValRaw::u32(callback.context),
            ValRaw::u32(event as u32),
            ValRaw::u32(handle),
            ValRaw::u32(result),
        ];
        unsafe {
            flags.set_may_enter(false);
            crate::Func::call_unchecked_raw(
                &mut store,
                callback.function.as_non_null(),
                params.as_mut_slice().into(),
            )?;
            flags.set_may_enter(true);
        }

        maybe_pop_call_context(&mut store, guest_task)?;

        let done = params[0].get_u32() != 0;
        log::trace!("{} done? {done}", guest_task.rep());
        if done {
            store.concurrent_state().table.get_mut(guest_task)?.callback = None;

            match &store.concurrent_state().table.get(guest_task)?.caller {
                Caller::Guest { task, .. } => {
                    let task = *task;
                    store =
                        maybe_send_event(store, task, Event::Done, AnyTask::Guest(guest_task), 0)?;
                }
                Caller::Host(_) => {
                    log::trace!("maybe_send_event will delete {}", guest_task.rep());
                    AnyTask::Guest(guest_task).delete_all_from(store.as_context_mut())?;
                }
            }
        }
        store.concurrent_state().guest_task = old_task;
        Ok(store)
    } else {
        store
            .concurrent_state()
            .table
            .get_mut(guest_task)?
            .events
            .push_back((event, call, result));

        let resumed = if event == Event::Done {
            if let Some((fiber, async_)) = store
                .concurrent_state()
                .table
                .get_mut(guest_task)?
                .deferred
                .take_stackful()
            {
                log::trace!(
                    "use fiber to deliver event {event:?} to {} for {}",
                    guest_task.rep(),
                    call.rep()
                );
                let old_task = store.concurrent_state().guest_task.replace(guest_task);
                store = resume_stackful(store, guest_task, fiber, async_)?;
                store.concurrent_state().guest_task = old_task;
                true
            } else {
                false
            }
        } else {
            false
        };

        if !resumed {
            log::trace!(
                "queue event {event:?} to {} for {}",
                guest_task.rep(),
                call.rep()
            );
        }

        Ok(store)
    }
}

fn resume_stackful<'a, T>(
    mut store: StoreContextMut<'a, T>,
    guest_task: TableId<GuestTask>,
    mut fiber: StoreFiber<'static>,
    async_: bool,
) -> Result<StoreContextMut<'a, T>> {
    maybe_push_call_context(&mut store, guest_task)?;

    let async_cx = AsyncCx::new(&mut store);

    let mut store = Some(store);
    loop {
        break match resume_fiber(&mut fiber, store.take(), Ok(()))? {
            Ok((mut store, result)) => {
                result?;
                if async_ {
                    if store
                        .concurrent_state()
                        .table
                        .get(guest_task)?
                        .lift_result
                        .is_some()
                    {
                        return Err(anyhow!(crate::Trap::NoAsyncResult));
                    }
                }
                if let Some(instance) = fiber.instance {
                    store = maybe_resume_next_task(store, instance)?;
                    for (event, call, _) in mem::take(
                        &mut store
                            .concurrent_state()
                            .table
                            .get_mut(guest_task)
                            .with_context(|| format!("bad handle: {}", guest_task.rep()))?
                            .events,
                    ) {
                        if event == Event::Done {
                            log::trace!("resume_stackful will delete call {}", call.rep());
                            call.delete_all_from(store.as_context_mut())?;
                        }
                    }
                    match &store.concurrent_state().table.get(guest_task)?.caller {
                        Caller::Host(_) => {
                            log::trace!("resume_stackful will delete task {}", guest_task.rep());
                            AnyTask::Guest(guest_task).delete_all_from(store.as_context_mut())?;
                            Ok(store)
                        }
                        Caller::Guest { task, .. } => {
                            let task = *task;
                            maybe_send_event(
                                store,
                                task,
                                Event::Done,
                                AnyTask::Guest(guest_task),
                                0,
                            )
                        }
                    }
                } else {
                    Ok(store)
                }
            }
            Err(new_store) => {
                if let Some(mut store) = new_store {
                    maybe_pop_call_context(&mut store, guest_task)?;
                    store.concurrent_state().table.get_mut(guest_task)?.deferred =
                        Deferred::Stackful { fiber, async_ };
                    Ok(store)
                } else {
                    // In this case, the fiber suspended while holding on to its
                    // `StoreContextMut` instead of returning it to us.  That
                    // means we can't do anything else with the store for now;
                    // the only thing we can do is suspend _our_ fiber back up
                    // to the top level executor, which will resume us when we
                    // can make more progress, at which point we'll loop around
                    // and restore the child fiber again.
                    unsafe { async_cx.suspend::<T>(None) }?;
                    continue;
                }
            }
        };
    }
}

fn resume_stackless<'a, T>(
    mut store: StoreContextMut<'a, T>,
    guest_task: TableId<GuestTask>,
    call: Box<dyn FnOnce(*mut dyn VMStore) -> Result<u32>>,
    instance: RuntimeComponentInstanceIndex,
    callback: SendSyncPtr<VMFuncRef>,
) -> Result<StoreContextMut<'a, T>> {
    maybe_push_call_context(&mut store, guest_task)?;

    let store = store.0.traitobj().as_ptr();
    let guest_context = call(store)?;
    let mut store = unsafe { StoreContextMut(&mut *store.cast()) };

    maybe_pop_call_context(&mut store, guest_task)?;

    let task = store.concurrent_state().table.get_mut(guest_task)?;
    let event = if task.lift_result.is_some() {
        if guest_context == 0 {
            return Err(anyhow!(crate::Trap::NoAsyncResult));
        }
        Event::Started
    } else if guest_context != 0 {
        Event::Returned
    } else {
        Event::Done
    };
    if guest_context != 0 {
        log::trace!("(resume_stackless) set callback for {}", guest_task.rep());
        task.callback = Some(Callback {
            function: callback,
            instance,
            context: guest_context,
        });
        for (event, call, result) in mem::take(&mut task.events) {
            store = maybe_send_event(store, guest_task, event, call, result)?;
        }
    }
    store = maybe_resume_next_task(store, instance)?;
    if let Caller::Guest { task, .. } = &store.concurrent_state().table.get(guest_task)?.caller {
        let task = *task;
        maybe_send_event(store, task, event, AnyTask::Guest(guest_task), 0)
    } else {
        Ok(store)
    }
}

fn poll_for_result<'a, T>(mut store: StoreContextMut<'a, T>) -> Result<StoreContextMut<'a, T>> {
    let task = store.concurrent_state().guest_task;
    poll_loop(store, move |store| {
        task.map(|task| {
            Ok::<_, anyhow::Error>(store.concurrent_state().table.get(task)?.result.is_none())
        })
        .unwrap_or(Ok(true))
    })
}

fn handle_ready<'a, T>(
    mut store: StoreContextMut<'a, T>,
    ready: Vec<HostTaskOutput>,
) -> Result<StoreContextMut<'a, T>> {
    for output in ready {
        match output {
            HostTaskOutput::Background(result) => {
                result?;
            }
            HostTaskOutput::Call { waitable, fun } => {
                let vm_store = store.0.traitobj().as_ptr();
                let result = fun(vm_store)?;
                store = unsafe { StoreContextMut::<T>(&mut *vm_store.cast()) };
                let task = match result.event {
                    Event::Done => AnyTask::Host(TableId::<HostTask>::new(waitable)),
                    Event::StreamRead
                    | Event::FutureRead
                    | Event::StreamWrite
                    | Event::FutureWrite => {
                        AnyTask::Transmit(TableId::<TransmitState>::new(waitable))
                    }
                    _ => unreachable!(),
                };
                store = maybe_send_event(store, result.caller, result.event, task, result.param)?;
            }
        }
    }
    Ok(store)
}

fn maybe_yield<'a, T>(mut store: StoreContextMut<'a, T>) -> Result<StoreContextMut<'a, T>> {
    let guest_task = store.concurrent_state().guest_task.unwrap();

    if store.concurrent_state().table.get(guest_task)?.should_yield {
        log::trace!("maybe_yield suspend {}", guest_task.rep());

        store.concurrent_state().yielding.insert(guest_task.rep());
        let cx = AsyncCx::new(&mut store);
        store = unsafe { cx.suspend(Some(store)) }?.unwrap();

        log::trace!("maybe_yield resume {}", guest_task.rep());
    } else {
        log::trace!("maybe_yield skip {}", guest_task.rep());
    }

    Ok(store)
}

fn unyield<'a, T>(mut store: StoreContextMut<'a, T>) -> Result<(StoreContextMut<'a, T>, bool)> {
    let mut resumed = false;
    for task in mem::take(&mut store.concurrent_state().yielding) {
        let guest_task = TableId::<GuestTask>::new(task);
        if let Some((fiber, async_)) = store
            .concurrent_state()
            .table
            .get_mut(guest_task)
            .with_context(|| format!("guest task {}", guest_task.rep()))?
            .deferred
            .take_stackful()
        {
            resumed = true;
            let old_task = store.concurrent_state().guest_task.replace(guest_task);
            store = resume_stackful(store, guest_task, fiber, async_)?;
            store.concurrent_state().guest_task = old_task;
        }
    }

    for instance in mem::take(&mut store.concurrent_state().unblocked) {
        let entry = store
            .concurrent_state()
            .instance_states
            .entry(instance)
            .or_default();

        if !(entry.backpressure || entry.in_sync_call) {
            if let Some(task) = entry.task_queue.pop_front() {
                resumed = true;
                store = resume(store, task)?;
            }
        }
    }

    Ok((store, resumed))
}

fn poll_loop<'a, T>(
    mut store: StoreContextMut<'a, T>,
    mut continue_: impl FnMut(&mut StoreContextMut<'a, T>) -> Result<bool>,
) -> Result<StoreContextMut<'a, T>> {
    loop {
        let cx = AsyncCx::new(&mut store);
        let mut future = pin!(store.concurrent_state().futures.next());
        let ready = unsafe { cx.poll(future.as_mut()) };

        match ready {
            Poll::Ready(Some(ready)) => {
                store = handle_ready(store, ready)?;
            }
            Poll::Ready(None) => {
                let (s, resumed) = unyield(store)?;
                store = s;
                if !resumed {
                    log::trace!("exhausted future queue; exiting poll_loop");
                    break;
                }
            }
            Poll::Pending => {
                let (s, resumed) = unyield(store)?;
                store = s;
                if continue_(&mut store)? {
                    let cx = AsyncCx::new(&mut store);
                    store = unsafe { cx.suspend(Some(store)) }?.unwrap();
                } else if !resumed {
                    break;
                }
            }
        }
    }

    Ok(store)
}

fn resume<'a, T>(
    mut store: StoreContextMut<'a, T>,
    task: TableId<GuestTask>,
) -> Result<StoreContextMut<'a, T>> {
    log::trace!("resume {}", task.rep());

    // TODO: Avoid calling `resume_stackful` or `resume_stackless` here, because it may call us, leading to
    // recursion limited only by the number of waiters.  Flatten this into an iteration instead.
    let old_task = store.concurrent_state().guest_task.replace(task);
    store = match mem::replace(
        &mut store.concurrent_state().table.get_mut(task)?.deferred,
        Deferred::None,
    ) {
        Deferred::None => unreachable!(),
        Deferred::Stackful { fiber, async_ } => resume_stackful(store, task, fiber, async_),
        Deferred::Stackless {
            call,
            instance,
            callback,
        } => resume_stackless(store, task, call, instance, callback),
    }?;
    store.concurrent_state().guest_task = old_task;
    Ok(store)
}

fn maybe_resume_next_task<'a, T>(
    mut store: StoreContextMut<'a, T>,
    instance: RuntimeComponentInstanceIndex,
) -> Result<StoreContextMut<'a, T>> {
    let state = store
        .concurrent_state()
        .instance_states
        .get_mut(&instance)
        .unwrap();

    if state.backpressure || state.in_sync_call {
        Ok(store)
    } else {
        if let Some(next) = state.task_queue.pop_front() {
            resume(store, next)
        } else {
            Ok(store)
        }
    }
}

struct StoreFiber<'a> {
    fiber: Option<
        Fiber<
            'a,
            (Option<*mut dyn VMStore>, Result<()>),
            Option<*mut dyn VMStore>,
            (Option<*mut dyn VMStore>, Result<()>),
        >,
    >,
    state: Option<AsyncWasmCallState>,
    engine: Engine,
    suspend: *mut *mut Suspend<
        (Option<*mut dyn VMStore>, Result<()>),
        Option<*mut dyn VMStore>,
        (Option<*mut dyn VMStore>, Result<()>),
    >,
    stack_limit: *mut usize,
    instance: Option<RuntimeComponentInstanceIndex>,
}

impl<'a> Drop for StoreFiber<'a> {
    fn drop(&mut self) {
        if !self.fiber.as_ref().unwrap().done() {
            let result = unsafe { resume_fiber_raw(self, None, Err(anyhow!("future dropped"))) };
            debug_assert!(result.is_ok());
        }

        self.state.take().unwrap().assert_null();

        unsafe {
            self.engine
                .allocator()
                .deallocate_fiber_stack(self.fiber.take().unwrap().into_stack());
        }
    }
}

unsafe impl<'a> Send for StoreFiber<'a> {}
unsafe impl<'a> Sync for StoreFiber<'a> {}

fn make_fiber<'a, T>(
    store: &mut StoreContextMut<T>,
    instance: Option<RuntimeComponentInstanceIndex>,
    fun: impl FnOnce(StoreContextMut<T>) -> Result<()> + 'a,
) -> Result<StoreFiber<'a>> {
    let engine = store.engine().clone();
    let stack = engine.allocator().allocate_fiber_stack()?;
    Ok(StoreFiber {
        fiber: Some(Fiber::new(
            stack,
            move |(store_ptr, result): (Option<*mut dyn VMStore>, Result<()>), suspend| {
                if result.is_err() {
                    (store_ptr, result)
                } else {
                    unsafe {
                        let store_ptr = store_ptr.unwrap();
                        let mut store = StoreContextMut(&mut *store_ptr.cast());
                        let suspend_ptr =
                            store.concurrent_state().async_state.current_suspend.get();
                        let _reset = Reset(suspend_ptr, *suspend_ptr);
                        *suspend_ptr = suspend;
                        (Some(store_ptr), fun(store.as_context_mut()))
                    }
                }
            },
        )?),
        state: Some(AsyncWasmCallState::new()),
        engine,
        suspend: store.concurrent_state().async_state.current_suspend.get(),
        stack_limit: store.0.runtime_limits().stack_limit.get(),
        instance,
    })
}

unsafe fn resume_fiber_raw<'a>(
    fiber: *mut StoreFiber<'a>,
    store: Option<*mut dyn VMStore>,
    result: Result<()>,
) -> Result<(Option<*mut dyn VMStore>, Result<()>), Option<*mut dyn VMStore>> {
    struct Restore<'a> {
        fiber: *mut StoreFiber<'a>,
        state: Option<PreviousAsyncWasmCallState>,
    }

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            unsafe {
                (*self.fiber).state = Some(self.state.take().unwrap().restore());
            }
        }
    }

    let _reset_suspend = Reset((*fiber).suspend, *(*fiber).suspend);
    let _reset_stack_limit = Reset((*fiber).stack_limit, *(*fiber).stack_limit);
    let state = Some((*fiber).state.take().unwrap().push());
    let restore = Restore { fiber, state };
    (*restore.fiber)
        .fiber
        .as_ref()
        .unwrap()
        .resume((store, result))
}

fn resume_fiber<'a, T>(
    fiber: &mut StoreFiber,
    store: Option<StoreContextMut<'a, T>>,
    result: Result<()>,
) -> Result<Result<(StoreContextMut<'a, T>, Result<()>), Option<StoreContextMut<'a, T>>>> {
    unsafe {
        match resume_fiber_raw(fiber, store.map(|s| s.0.traitobj().as_ptr()), result)
            .map(|(store, result)| (StoreContextMut(&mut *store.unwrap().cast()), result))
            .map_err(|v| v.map(|v| StoreContextMut(&mut *v.cast())))
        {
            Ok(pair) => Ok(Ok(pair)),
            Err(s) => {
                if let Some(range) = fiber.fiber.as_ref().unwrap().stack().range() {
                    AsyncWasmCallState::assert_current_state_not_in_range(range);
                }

                Ok(Err(s))
            }
        }
    }
}

unsafe fn suspend_fiber<'a, T>(
    suspend: *mut *mut Suspend<
        (Option<*mut dyn VMStore>, Result<()>),
        Option<*mut dyn VMStore>,
        (Option<*mut dyn VMStore>, Result<()>),
    >,
    stack_limit: *mut usize,
    store: Option<StoreContextMut<'a, T>>,
) -> Result<Option<StoreContextMut<'a, T>>> {
    let _reset_suspend = Reset(suspend, *suspend);
    let _reset_stack_limit = Reset(stack_limit, *stack_limit);
    assert!(!(*suspend).is_null());
    let (store, result) = (**suspend).suspend(store.map(|s| s.0.traitobj().as_ptr()));
    result?;
    Ok(store.map(|v| StoreContextMut(&mut *v.cast())))
}

enum TaskCheck {
    Wait(*mut VMMemoryDefinition, u32, RuntimeComponentInstanceIndex),
    Poll(*mut VMMemoryDefinition, u32, RuntimeComponentInstanceIndex),
    Yield,
}

fn task_check<T>(
    mut cx: StoreContextMut<T>,
    instance: *mut ComponentInstance,
    async_: bool,
    check: TaskCheck,
) -> Result<u32> {
    if async_ {
        bail!("todo: async `task.wait`, `task.poll`, and `task.yield` not yet implemented");
    }

    let guest_task = cx.concurrent_state().guest_task.unwrap();

    log::trace!("task check for {}", guest_task.rep());

    let wait = matches!(check, TaskCheck::Wait(..));

    if wait
        && cx
            .concurrent_state()
            .table
            .get(guest_task)?
            .callback
            .is_some()
    {
        bail!("cannot call `task.wait` from async-lifted export with callback");
    }

    if matches!(check, TaskCheck::Yield)
        || cx
            .concurrent_state()
            .table
            .get(guest_task)?
            .events
            .is_empty()
    {
        cx = maybe_yield(cx)?;

        if cx
            .concurrent_state()
            .table
            .get(guest_task)?
            .events
            .is_empty()
        {
            cx = poll_loop(cx, move |cx| {
                Ok::<_, anyhow::Error>(
                    wait && cx
                        .concurrent_state()
                        .table
                        .get(guest_task)?
                        .events
                        .is_empty(),
                )
            })?;
        }
    }

    log::trace!("task check for {}, part two", guest_task.rep());

    let result = match check {
        TaskCheck::Wait(memory, payload, caller_instance) => {
            let (event, call, result) = cx
                .concurrent_state()
                .table
                .get_mut(guest_task)?
                .events
                .pop_front()
                .ok_or_else(|| anyhow!("no tasks to wait for"))?;

            log::trace!(
                "deliver event {event:?} via task.wait to {} for {}",
                guest_task.rep(),
                call.rep()
            );

            let entry = unsafe {
                (*instance).component_waitable_tables()[caller_instance].get_mut_by_rep(call.rep())
            };
            let Some((handle, _)) = entry else {
                bail!("handle not found for waitable rep {}", call.rep());
            };

            let options = unsafe {
                Options::new(
                    cx.0.id(),
                    NonNull::new(memory),
                    None,
                    StringEncoding::Utf8,
                    true,
                    None,
                )
            };
            let types = unsafe { (*instance).component_types() };
            let ptr =
                func::validate_inbounds::<u32>(options.memory_mut(cx.0), &ValRaw::u32(payload))?;
            let mut lower = unsafe { LowerContext::new(cx, &options, types, instance) };
            handle.store(&mut lower, InterfaceType::U32, ptr)?;
            result.store(&mut lower, InterfaceType::U32, ptr + 4)?;

            Ok(event as u32)
        }
        TaskCheck::Poll(memory, payload, caller_instance) => {
            if let Some((event, call, result)) = cx
                .concurrent_state()
                .table
                .get_mut(guest_task)?
                .events
                .pop_front()
            {
                let entry = unsafe {
                    (*instance).component_waitable_tables()[caller_instance]
                        .get_mut_by_rep(call.rep())
                };
                let Some((handle, _)) = entry else {
                    bail!("handle not found for waitable rep {}", call.rep());
                };

                let options = unsafe {
                    Options::new(
                        cx.0.id(),
                        NonNull::new(memory),
                        None,
                        StringEncoding::Utf8,
                        true,
                        None,
                    )
                };
                let types = unsafe { (*instance).component_types() };
                let ptr = func::validate_inbounds::<(u32, u32)>(
                    options.memory_mut(cx.0),
                    &ValRaw::u32(payload),
                )?;
                let mut lower = unsafe { LowerContext::new(cx, &options, types, instance) };
                (event as u32).store(&mut lower, InterfaceType::U32, ptr)?;
                handle.store(&mut lower, InterfaceType::U32, ptr + 4)?;
                result.store(&mut lower, InterfaceType::U32, ptr + 8)?;

                Ok(1)
            } else {
                log::trace!(
                    "no events ready to deliver via task.poll to {}",
                    guest_task.rep()
                );

                Ok(0)
            }
        }
        TaskCheck::Yield => Ok(0),
    };

    result
}

fn may_enter<T>(
    store: &mut StoreContextMut<T>,
    mut guest_task: TableId<GuestTask>,
    guest_instance: RuntimeComponentInstanceIndex,
) -> bool {
    // Walk the task tree back to the root, looking for potential reentrance.
    //
    // TODO: This could be optimized by maintaining a per-`GuestTask` bitset
    // such that each bit represents and instance which has been entered by that
    // task or an ancestor of that task, in which case this would be a constant
    // time check.
    loop {
        match &store
            .concurrent_state()
            .table
            .get_mut(guest_task)
            .unwrap()
            .caller
        {
            Caller::Host(_) => break true,
            Caller::Guest { task, instance } => {
                if *instance == guest_instance {
                    break false;
                } else {
                    guest_task = *task;
                }
            }
        }
    }
}

fn make_call<T>(
    guest_task: TableId<GuestTask>,
    callee: SendSyncPtr<VMFuncRef>,
    callee_instance: RuntimeComponentInstanceIndex,
    param_count: usize,
    result_count: usize,
    flags: Option<InstanceFlags>,
) -> impl FnOnce(
    StoreContextMut<T>,
) -> Result<([MaybeUninit<ValRaw>; MAX_FLAT_PARAMS], StoreContextMut<T>)>
       + Send
       + Sync
       + 'static {
    move |mut cx: StoreContextMut<T>| {
        if !may_enter(&mut cx, guest_task, callee_instance) {
            bail!(crate::Trap::CannotEnterComponent);
        }

        let mut storage = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
        let lower = cx
            .concurrent_state()
            .table
            .get_mut(guest_task)?
            .lower_params
            .take()
            .unwrap();
        let cx = cx.0.traitobj().as_ptr();
        lower(cx, &mut storage[..param_count])?;
        let mut cx = unsafe { StoreContextMut::<T>(&mut *cx.cast()) };

        unsafe {
            if let Some(mut flags) = flags {
                flags.set_may_enter(false);
            }
            crate::Func::call_unchecked_raw(
                &mut cx,
                callee.as_non_null(),
                NonNull::new(
                    &mut storage[..param_count.max(result_count)] as *mut [MaybeUninit<ValRaw>]
                        as _,
                )
                .unwrap(),
            )?;
            if let Some(mut flags) = flags {
                flags.set_may_enter(true);
            }
        }

        Ok((storage, cx))
    }
}

fn do_start_call<'a, T>(
    mut cx: StoreContextMut<'a, T>,
    instance: *mut ComponentInstance,
    guest_task: TableId<GuestTask>,
    async_: bool,
    call: impl FnOnce(
            StoreContextMut<T>,
        ) -> Result<([MaybeUninit<ValRaw>; MAX_FLAT_PARAMS], StoreContextMut<T>)>
        + Send
        + Sync
        + 'static,
    callback: Option<SendSyncPtr<VMFuncRef>>,
    post_return: Option<SendSyncPtr<VMFuncRef>>,
    callee_instance: RuntimeComponentInstanceIndex,
    result_count: usize,
) -> Result<(u32, StoreContextMut<'a, T>)> {
    let state = &mut cx
        .concurrent_state()
        .instance_states
        .entry(callee_instance)
        .or_default();
    let ready = state.task_queue.is_empty() && !(state.backpressure || state.in_sync_call);

    let mut guest_context = 0;
    let mut async_finished = false;

    let mut cx = if let Some(callback) = callback {
        assert!(async_);

        if ready {
            maybe_push_call_context(&mut cx, guest_task)?;
            let (storage, mut cx) = call(cx)?;
            guest_context = unsafe { storage[0].assume_init() }.get_i32() as u32;
            async_finished = guest_context == 0;
            maybe_pop_call_context(&mut cx, guest_task)?;
            cx
        } else {
            cx.concurrent_state()
                .instance_states
                .get_mut(&callee_instance)
                .unwrap()
                .task_queue
                .push_back(guest_task);

            cx.concurrent_state().table.get_mut(guest_task)?.deferred = Deferred::Stackless {
                call: Box::new(move |cx| {
                    let mut cx = unsafe { StoreContextMut(&mut *cx.cast()) };
                    let old_task = cx.concurrent_state().guest_task.replace(guest_task);
                    let (storage, mut cx) = call(cx)?;
                    cx.concurrent_state().guest_task = old_task;
                    Ok(unsafe { storage[0].assume_init() }.get_i32() as u32)
                }),
                instance: callee_instance,
                callback,
            };
            cx
        }
    } else {
        let mut fiber = make_fiber(&mut cx, Some(callee_instance), move |mut cx| {
            let mut flags = unsafe { (*instance).instance_flags(callee_instance) };

            if !async_ {
                cx.concurrent_state()
                    .instance_states
                    .get_mut(&callee_instance)
                    .unwrap()
                    .in_sync_call = true;
            }

            let (storage, mut cx) = call(cx)?;

            if !async_ {
                cx.concurrent_state()
                    .instance_states
                    .get_mut(&callee_instance)
                    .unwrap()
                    .in_sync_call = false;

                let (lift, _) = cx
                    .concurrent_state()
                    .table
                    .get_mut(guest_task)?
                    .lift_result
                    .take()
                    .unwrap();

                assert!(cx
                    .concurrent_state()
                    .table
                    .get(guest_task)?
                    .result
                    .is_none());

                let cx = cx.0.traitobj().as_ptr();
                let result = lift(cx, unsafe {
                    mem::transmute::<&[MaybeUninit<ValRaw>], &[ValRaw]>(&storage[..result_count])
                })?;
                let mut cx = unsafe { StoreContextMut::<T>(&mut *cx.cast()) };

                unsafe { flags.set_needs_post_return(false) }

                if let Some(func) = post_return {
                    let arg = match result_count {
                        0 => ValRaw::i32(0),
                        1 => unsafe { storage[0].assume_init() },
                        _ => unreachable!(),
                    };
                    unsafe {
                        crate::Func::call_unchecked_raw(
                            &mut cx,
                            func.as_non_null(),
                            NonNull::new(ptr::slice_from_raw_parts(&arg, 1).cast_mut()).unwrap(),
                        )?;
                    }
                }

                unsafe { flags.set_may_enter(true) }

                let (calls, host_table, _) = cx.0.component_resource_state();
                ResourceTables {
                    calls,
                    host_table: Some(host_table),
                    tables: unsafe { Some((*instance).component_resource_tables()) },
                }
                .exit_call()?;

                if let Caller::Host(tx) =
                    &mut cx.concurrent_state().table.get_mut(guest_task)?.caller
                {
                    _ = tx.take().unwrap().send(result);
                } else {
                    cx.concurrent_state().table.get_mut(guest_task)?.result = Some(result);
                }
            }

            Ok(())
        })?;

        cx.concurrent_state()
            .table
            .get_mut(guest_task)?
            .should_yield = true;

        if ready {
            maybe_push_call_context(&mut cx, guest_task)?;
            let mut cx = Some(cx);
            loop {
                match resume_fiber(&mut fiber, cx.take(), Ok(()))? {
                    Ok((cx, result)) => {
                        async_finished = async_;
                        result?;
                        break maybe_resume_next_task(cx, callee_instance)?;
                    }
                    Err(cx) => {
                        if let Some(mut cx) = cx {
                            maybe_pop_call_context(&mut cx, guest_task)?;
                            cx.concurrent_state().table.get_mut(guest_task)?.deferred =
                                Deferred::Stackful { fiber, async_ };
                            break cx;
                        } else {
                            unsafe { suspend_fiber::<T>(fiber.suspend, fiber.stack_limit, None)? };
                        }
                    }
                }
            }
        } else {
            cx.concurrent_state()
                .instance_states
                .get_mut(&callee_instance)
                .unwrap()
                .task_queue
                .push_back(guest_task);

            cx.concurrent_state().table.get_mut(guest_task)?.deferred =
                Deferred::Stackful { fiber, async_ };
            cx
        }
    };

    let guest_task = cx.concurrent_state().guest_task.take().unwrap();

    let caller =
        if let Caller::Guest { task, .. } = &cx.concurrent_state().table.get(guest_task)?.caller {
            Some(*task)
        } else {
            None
        };
    cx.concurrent_state().guest_task = caller;

    let task = cx.concurrent_state().table.get_mut(guest_task)?;

    if guest_context != 0 {
        log::trace!("(do_start_call) set callback for {}", guest_task.rep());
        task.callback = Some(Callback {
            function: callback.unwrap(),
            instance: callee_instance,
            context: guest_context,
        });
        for (event, call, result) in mem::take(&mut task.events) {
            cx = maybe_send_event(cx, guest_task, event, call, result)?;
        }
    } else if async_finished
        && !(matches!(&task.caller, Caller::Guest {..} if task.result.is_some())
            || matches!(&task.caller, Caller::Host(tx) if tx.is_none()))
    {
        return Err(anyhow!(crate::Trap::NoAsyncResult));
    }

    Ok((guest_context, cx))
}

pub(crate) fn start_call<'a, T: Send, LowerParams: Copy, R: 'static>(
    mut store: StoreContextMut<'a, T>,
    lower_params: LowerFn,
    lower_context: LiftLowerContext,
    lift_result: LiftFn,
    lift_context: LiftLowerContext,
    handle: Func,
) -> Result<(Promise<R>, StoreContextMut<'a, T>)> {
    let func_data = &store.0[handle.0];
    let task_return_type = func_data.types[func_data.ty].results;
    let is_concurrent = func_data.options.async_();
    let component_instance = func_data.component_instance;
    let instance = func_data.instance;
    let callee = func_data.export.func_ref;
    let callback = func_data.options.callback;
    let post_return = func_data.post_return;

    assert!(store.concurrent_state().guest_task.is_none());

    // TODO: Can we safely leave this set?  Can the same store be used with more than one ComponentInstance?  Could
    // we instead set this when the ConcurrentState is created so we don't have to set/unset it on the fly?
    store.concurrent_state().component_instance =
        Some(store.0[instance.0].as_ref().unwrap().state.ptr);

    let (tx, rx) = oneshot::channel();

    let guest_task = store.concurrent_state().table.push(GuestTask {
        lower_params: Some(Box::new(for_any_lower(move |store, params| {
            lower_params(lower_context, store, params)
        })) as RawLower),
        lift_result: Some((
            Box::new(for_any_lift(move |store, result| {
                lift_result(lift_context, store, result)
            })) as RawLift,
            task_return_type,
        )),
        caller: Caller::Host(Some(tx)),
        ..GuestTask::default()
    })?;

    log::trace!("starting call {}", guest_task.rep());

    let instance = store.0[instance.0].as_ref().unwrap().instance_ptr();

    let call = make_call(
        guest_task,
        SendSyncPtr::new(callee),
        component_instance,
        mem::size_of::<LowerParams>() / mem::size_of::<ValRaw>(),
        1,
        if callback.is_none() {
            None
        } else {
            Some(unsafe { (*instance).instance_flags(component_instance) })
        },
    );

    store.concurrent_state().guest_task = Some(guest_task);

    store = do_start_call(
        store,
        instance,
        guest_task,
        is_concurrent,
        call,
        callback.map(SendSyncPtr::new),
        post_return.map(|f| SendSyncPtr::new(f.func_ref)),
        component_instance,
        1,
    )?
    .1;

    store.concurrent_state().guest_task = None;

    log::trace!("started call {}", guest_task.rep());

    Ok((
        Promise(Box::pin(
            rx.map(|result| *result.unwrap().downcast().unwrap()),
        )),
        store,
    ))
}

pub(crate) fn call<'a, T: Send, LowerParams: Copy, R: Send + Sync + 'static>(
    store: StoreContextMut<'a, T>,
    lower_params: LowerFn,
    lower_context: LiftLowerContext,
    lift_result: LiftFn,
    lift_context: LiftLowerContext,
    handle: Func,
) -> Result<(R, StoreContextMut<'a, T>)> {
    let (promise, store) = start_call::<_, LowerParams, R>(
        store,
        lower_params,
        lower_context,
        lift_result,
        lift_context,
        handle,
    )?;

    loop_until(store, promise.into_future())
}

fn loop_until<'a, T, R>(
    mut store: StoreContextMut<'a, T>,
    mut future: Pin<Box<dyn Future<Output = R> + Send + '_>>,
) -> Result<(R, StoreContextMut<'a, T>)> {
    let result = Arc::new(Mutex::new(None));
    let poll = &mut {
        let result = result.clone();
        move |store: &mut StoreContextMut<T>| {
            let cx = AsyncCx::new(store);
            let ready = unsafe { cx.poll(future.as_mut()) };
            Ok(match ready {
                Poll::Ready(value) => {
                    *result.lock().unwrap() = Some(value);
                    false
                }
                Poll::Pending => true,
            })
        }
    };
    store = poll_loop(store, |store| poll(store))?;

    // Poll once more to get the result if necessary:
    if result.lock().unwrap().is_none() {
        poll(&mut store)?;
    }

    let result = result.lock().unwrap().take();
    if let Some(result) = result {
        Ok((result, store))
    } else {
        // All outstanding host tasks completed, but the guest never yielded a result.
        Err(anyhow!(crate::Trap::NoAsyncResult))
    }
}

async fn poll_until<'a, T: Send, R: Send + Sync + 'static>(
    store: StoreContextMut<'a, T>,
    future: Pin<Box<dyn Future<Output = R> + Send + '_>>,
) -> Result<(StoreContextMut<'a, T>, R)> {
    let (result, store) = on_fiber(store, None, move |store| {
        Ok::<_, anyhow::Error>(loop_until(store.as_context_mut(), future)?.0)
    })
    .await?;

    Ok((store, result?))
}

async fn poll_fn<'a, T, R>(
    mut store: StoreContextMut<'a, T>,
    guard_range: (Option<SendSyncPtr<u8>>, Option<SendSyncPtr<u8>>),
    mut fun: impl FnMut(
        &mut Context,
        Option<StoreContextMut<'a, T>>,
    ) -> Result<R, Option<StoreContextMut<'a, T>>>,
) -> R {
    #[derive(Clone, Copy)]
    struct PollCx(*mut PollContext);

    unsafe impl Send for PollCx {}

    let poll_cx = PollCx(store.concurrent_state().async_state.current_poll_cx.get());
    future::poll_fn({
        let mut store = Some(store);

        move |cx| unsafe {
            let _reset = Reset(poll_cx.0, *poll_cx.0);
            let guard_range_start = guard_range.0.map(|v| v.as_ptr()).unwrap_or(ptr::null_mut());
            let guard_range_end = guard_range.1.map(|v| v.as_ptr()).unwrap_or(ptr::null_mut());
            *poll_cx.0 = PollContext {
                future_context: mem::transmute::<&mut Context<'_>, *mut Context<'static>>(cx),
                guard_range_start,
                guard_range_end,
            };
            #[allow(dropping_copy_types)]
            drop(poll_cx);

            match fun(cx, store.take()) {
                Ok(v) => Poll::Ready(v),
                Err(s) => {
                    store = s;
                    Poll::Pending
                }
            }
        }
    })
    .await
}

enum CallerInfo {
    Async {
        params: u32,
        results: u32,
    },
    Sync {
        params: Vec<ValRaw>,
        result_count: u32,
    },
}

fn enter_call<T>(
    mut cx: StoreContextMut<T>,
    start: *mut VMFuncRef,
    return_: *mut VMFuncRef,
    caller_instance: RuntimeComponentInstanceIndex,
    task_return_type: TypeTupleIndex,
    caller_info: CallerInfo,
) -> Result<()> {
    enum ResultInfo {
        Heap { results: u32 },
        Stack { result_count: u32 },
    }

    let result_info = match &caller_info {
        CallerInfo::Async { results, .. } => ResultInfo::Heap { results: *results },
        CallerInfo::Sync {
            result_count,
            params,
        } if *result_count > u32::try_from(MAX_FLAT_RESULTS).unwrap() => ResultInfo::Heap {
            results: params.last().unwrap().get_u32(),
        },
        CallerInfo::Sync { result_count, .. } => ResultInfo::Stack {
            result_count: *result_count,
        },
    };

    let start = SendSyncPtr::new(NonNull::new(start).unwrap());
    let return_ = SendSyncPtr::new(NonNull::new(return_).unwrap());
    let old_task = cx.concurrent_state().guest_task.take();
    let old_task_rep = old_task.map(|v| v.rep());
    let new_task = GuestTask {
        lower_params: Some(Box::new(move |cx, dst| {
            let mut cx = unsafe { StoreContextMut::<T>(&mut *cx.cast()) };
            assert!(dst.len() <= MAX_FLAT_PARAMS);
            let mut src = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
            let count = match caller_info {
                CallerInfo::Async { params, .. } => {
                    src[0] = MaybeUninit::new(ValRaw::u32(params));
                    1
                }
                CallerInfo::Sync { params, .. } => {
                    src[..params.len()].copy_from_slice(unsafe {
                        mem::transmute::<&[ValRaw], &[MaybeUninit<ValRaw>]>(&params)
                    });
                    params.len()
                }
            };
            unsafe {
                crate::Func::call_unchecked_raw(
                    &mut cx,
                    start.as_non_null(),
                    NonNull::new(
                        &mut src[..count.max(dst.len())] as *mut [MaybeUninit<ValRaw>] as _,
                    )
                    .unwrap(),
                )?;
            }
            dst.copy_from_slice(&src[..dst.len()]);
            let task = cx.concurrent_state().guest_task.unwrap();
            if let Some(rep) = old_task_rep {
                maybe_send_event(
                    cx,
                    TableId::new(rep),
                    Event::Started,
                    AnyTask::Guest(task),
                    0,
                )?;
            }
            Ok(())
        })),
        lift_result: Some((
            Box::new(move |cx, src| {
                let mut cx = unsafe { StoreContextMut::<T>(&mut *cx.cast()) };
                let mut my_src = src.to_owned(); // TODO: use stack to avoid allocation?
                if let ResultInfo::Heap { results } = &result_info {
                    my_src.push(ValRaw::u32(*results));
                }
                unsafe {
                    crate::Func::call_unchecked_raw(
                        &mut cx,
                        return_.as_non_null(),
                        my_src.as_mut_slice().into(),
                    )?;
                }
                let task = cx.concurrent_state().guest_task.unwrap();
                if let ResultInfo::Stack { result_count } = &result_info {
                    match result_count {
                        0 => {}
                        1 => {
                            cx.concurrent_state().table.get_mut(task)?.sync_result =
                                Some(my_src[0]);
                        }
                        _ => unreachable!(),
                    }
                }
                if let Some(rep) = old_task_rep {
                    maybe_send_event(
                        cx,
                        TableId::new(rep),
                        Event::Returned,
                        AnyTask::Guest(task),
                        0,
                    )?;
                }
                Ok(Box::new(DummyResult) as Box<dyn std::any::Any + Send + Sync>)
            }),
            task_return_type,
        )),
        result: None,
        callback: None,
        caller: Caller::Guest {
            task: old_task.unwrap(),
            instance: caller_instance,
        },
        deferred: Deferred::None,
        events: VecDeque::new(),
        should_yield: false,
        call_context: Some(CallContext::default()),
        sync_result: None,
    };
    let guest_task = if let Some(old_task) = old_task {
        let child = cx.concurrent_state().table.push_child(new_task, old_task)?;
        log::trace!("new child of {}: {}", old_task.rep(), child.rep());
        child
    } else {
        cx.concurrent_state().table.push(new_task)?
    };

    cx.concurrent_state().guest_task = Some(guest_task);

    Ok(())
}

fn exit_call<T>(
    mut cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
    callback: *mut VMFuncRef,
    post_return: *mut VMFuncRef,
    caller_instance: RuntimeComponentInstanceIndex,
    callee: *mut VMFuncRef,
    callee_instance: RuntimeComponentInstanceIndex,
    param_count: u32,
    result_count: u32,
    flags: u32,
    storage: Option<&mut [MaybeUninit<ValRaw>]>,
) -> Result<u32> {
    let async_caller = storage.is_none();
    let guest_task = cx.concurrent_state().guest_task.unwrap();
    let callee = SendSyncPtr::new(NonNull::new(callee).unwrap());
    let param_count = usize::try_from(param_count).unwrap();
    assert!(param_count <= MAX_FLAT_PARAMS);
    let result_count = usize::try_from(result_count).unwrap();
    assert!(result_count <= MAX_FLAT_RESULTS);

    let call = make_call(
        guest_task,
        callee,
        callee_instance,
        param_count,
        result_count,
        if callback.is_null() {
            None
        } else {
            Some(instance.instance_flags(callee_instance))
        },
    );

    let (guest_context, new_cx) = do_start_call(
        cx,
        instance,
        guest_task,
        (flags & EXIT_FLAG_ASYNC_CALLEE) != 0,
        call,
        NonNull::new(callback).map(SendSyncPtr::new),
        NonNull::new(post_return).map(SendSyncPtr::new),
        callee_instance,
        result_count,
    )?;

    cx = new_cx;

    let task = cx.concurrent_state().table.get(guest_task)?;

    let mut status = if task.lower_params.is_some() {
        Status::Starting
    } else if task.lift_result.is_some() {
        Status::Started
    } else if guest_context != 0 || callback.is_null() {
        Status::Returned
    } else {
        Status::Done
    };

    log::trace!("status {status:?} for {}", guest_task.rep());

    let call = if status != Status::Done {
        if async_caller {
            instance.component_waitable_tables()[caller_instance]
                .insert(guest_task.rep(), WaitableState::Task)?
        } else {
            cx = poll_for_result(cx)?;
            status = Status::Done;
            0
        }
    } else {
        0
    };

    if let Some(storage) = storage {
        if let Some(result) = cx
            .concurrent_state()
            .table
            .get_mut(guest_task)?
            .sync_result
            .take()
        {
            storage[0] = MaybeUninit::new(result);
        }
        Ok(0)
    } else {
        Ok(((status as u32) << 30) | call)
    }
}
