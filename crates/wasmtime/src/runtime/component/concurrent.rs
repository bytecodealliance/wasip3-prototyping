use {
    crate::{
        component::{
            func::{self, Func, Options},
            Instance, Lift, Lower, Val,
        },
        store::{StoreId, StoreInner, StoreOpaque},
        vm::{
            component::{CallContext, ComponentInstance, InstanceFlags, ResourceTables},
            mpk::{self, ProtectionMask},
            AsyncWasmCallState, PreviousAsyncWasmCallState, SendSyncPtr, VMFuncRef,
            VMMemoryDefinition, VMStore, VMStoreRawPtr,
        },
        AsContext, AsContextMut, Engine, StoreContext, StoreContextMut, ValRaw,
    },
    anyhow::{anyhow, bail, Context as _, Result},
    error_contexts::{GlobalErrorContextRefCount, LocalErrorContextRefCount},
    futures::{
        channel::oneshot,
        future::{self, FutureExt},
        stream::{FuturesUnordered, StreamExt},
    },
    futures_and_streams::{FlatAbi, StreamFutureState, TableIndex, TransmitHandle},
    once_cell::sync::Lazy,
    ready_chunks::ReadyChunks,
    states::StateTable,
    std::{
        any::Any,
        borrow::ToOwned,
        boxed::Box,
        cell::{RefCell, UnsafeCell},
        collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
        future::Future,
        marker::PhantomData,
        mem::{self, MaybeUninit},
        ops::{Deref, DerefMut, Range},
        pin::{pin, Pin},
        ptr::{self, NonNull},
        sync::{
            atomic::{AtomicU32, Ordering::Relaxed},
            Arc, Mutex,
        },
        task::{Context, Poll, Wake, Waker},
        vec::Vec,
    },
    table::{Table, TableError, TableId},
    wasmtime_environ::{
        component::{
            RuntimeComponentInstanceIndex, StringEncoding,
            TypeComponentGlobalErrorContextTableIndex, TypeComponentLocalErrorContextTableIndex,
            TypeFutureTableIndex, TypeStreamTableIndex, TypeTupleIndex, MAX_FLAT_PARAMS,
            MAX_FLAT_RESULTS,
        },
        fact, PrimaryMap,
    },
    wasmtime_fiber::{Fiber, Suspend},
};

pub(crate) use futures_and_streams::{
    lower_error_context_to_index, lower_future_to_index, lower_stream_to_index, ResourcePair,
};
pub use futures_and_streams::{
    BytesBuffer, BytesMutBuffer, ErrorContext, FutureReader, FutureWriter, HostFuture, HostStream,
    ReadBuffer, Single, StreamReader, StreamWriter, VecBuffer, Watch, WriteBuffer,
};

mod error_contexts;
mod futures_and_streams;
mod ready_chunks;
mod states;
mod table;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
#[repr(u32)]
enum Status {
    Starting = 1,
    Started,
    Returned,
}

#[derive(Clone, Copy, Debug)]
enum Event {
    None,
    _CallStarting,
    CallStarted,
    CallReturned,
    StreamRead {
        count: u32,
        handle: u32,
        ty: TypeStreamTableIndex,
    },
    StreamWrite {
        count: u32,
        pending: Option<(TypeStreamTableIndex, u32)>,
    },
    FutureRead {
        count: u32,
        handle: u32,
        ty: TypeFutureTableIndex,
    },
    FutureWrite {
        count: u32,
        pending: Option<(TypeFutureTableIndex, u32)>,
    },
}

impl Event {
    fn parts(self) -> (u32, u32) {
        match self {
            Event::None => (0, 0),
            Event::_CallStarting => (1, 0),
            Event::CallStarted => (2, 0),
            Event::CallReturned => (3, 0),
            Event::StreamRead { count, .. } => (5, count),
            Event::StreamWrite { count, .. } => (6, count),
            Event::FutureRead { count, .. } => (7, count),
            Event::FutureWrite { count, .. } => (8, count),
        }
    }
}

mod callback_code {
    pub const EXIT: u32 = 0;
    pub const YIELD: u32 = 1;
    pub const WAIT: u32 = 2;
    pub const POLL: u32 = 3;
}

const EXIT_FLAG_ASYNC_CALLEE: u32 = fact::EXIT_FLAG_ASYNC_CALLEE as u32;

/// Represents the result of a concurrent operation.
///
/// This is similar to a [`std::future::Future`] except that it represents an
/// operation which requires exclusive access to a store in order to make
/// progress -- without monopolizing that store for the lifetime of the
/// operation.
///
/// See also `Instance::promise` for wrapping an arbitrary `Future` in a
/// `Promise` so it can be polled as part of an `Instance`'s event loop.
pub struct Promise<T> {
    inner: Pin<Box<dyn Future<Output = T> + Send + 'static>>,
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
}

impl<T: Send + Sync + 'static> Promise<T> {
    /// Map the result of this `Promise` from one value to another.
    pub fn map<U>(self, fun: impl FnOnce(T) -> U + Send + Sync + 'static) -> Promise<U> {
        Promise {
            inner: Box::pin(self.inner.map(fun)),
            instance: self.instance,
            id: self.id,
        }
    }

    /// Convert this `Promise` to a future which may be `await`ed for its
    /// result.
    ///
    /// This will poll the inner `Future` as part of the associated `Instance`'s
    /// event loop, ensuring that any other tasks managed by that event loop
    /// (which the `Future` might depend on directly or indirectly) make
    /// progress concurrently.
    ///
    /// The returned future will require exclusive use of the store until it
    /// completes.  If you need to await more than one `Promise` concurrently,
    /// use [`PromisesUnordered`].
    pub async fn get<U: Send>(self, mut store: impl AsContextMut<Data = U>) -> Result<T> {
        let store = store.as_context_mut();
        assert_eq!(store.0.id(), self.id);
        unsafe { &mut *self.instance.as_ptr() }
            .poll_until(store, self.inner)
            .await
    }

    /// Convert this `Promise` to a future which may be `await`ed for its
    /// result.
    ///
    /// Unlike [`Self::get`], this does _not_ take a store parameter, meaning
    /// the returned future will not make progress until and unless the event
    /// loop for the `Instance` it came from is polled.  Thus, this method
    /// should only be used from within host functions and not from top-level
    /// embedder code unless that top-level code is itself wrapped in a
    /// `Promise` using `Instance::promise`.
    pub fn into_future(self) -> Pin<Box<dyn Future<Output = T> + Send + 'static>> {
        self.inner
    }
}

/// Represents a collection of zero or more concurrent operations.
///
/// Similar to [`futures::stream::FuturesUnordered`], this type supports
/// `await`ing more than one [`Promise`]s concurrently.
pub struct PromisesUnordered<T> {
    inner: FuturesUnordered<Pin<Box<dyn Future<Output = T> + Send + 'static>>>,
    instance: Option<(SendSyncPtr<ComponentInstance>, StoreId)>,
}

impl<T: Send + Sync + 'static> PromisesUnordered<T> {
    /// Create a new `PromisesUnordered` with no entries.
    pub fn new() -> Self {
        Self {
            inner: FuturesUnordered::new(),
            instance: None,
        }
    }

    /// Add the specified [`Promise`] to this collection.
    pub fn push(&mut self, promise: Promise<T>) {
        if let Some((instance, id)) = self.instance {
            assert_eq!(instance, promise.instance);
            assert_eq!(id, promise.id);
        } else {
            self.instance = Some((promise.instance, promise.id));
        }
        self.inner.push(promise.inner)
    }

    /// Get the next result from this collection, if any.
    pub async fn next<U: Send>(
        &mut self,
        mut store: impl AsContextMut<Data = U>,
    ) -> Result<Option<T>> {
        if let Some((instance, id)) = self.instance {
            let store = store.as_context_mut();
            assert_eq!(store.0.id(), id);
            unsafe { &mut *instance.as_ptr() }
                .poll_until(
                    store,
                    Box::pin(self.inner.next())
                        as Pin<Box<dyn Future<Output = Option<T>> + Send + '_>>,
                )
                .await
        } else {
            Ok(None)
        }
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

    /// Retrieve the component instance of the caller.
    pub fn instance(&self) -> Instance {
        self.0.instance()
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
    instance: Option<Instance>,
    _phantom: PhantomData<fn() -> (*mut U, *mut StoreInner<T>)>,
}

unsafe impl<T, U> Send for Accessor<T, U> {}
unsafe impl<T, U> Sync for Accessor<T, U> {}

impl<T, U> Accessor<T, U> {
    #[doc(hidden)]
    pub unsafe fn new(
        get: fn() -> (*mut u8, *mut u8),
        spawn: fn(Spawned),
        instance: Option<Instance>,
    ) -> Self {
        Self {
            get: Arc::new(get),
            spawn,
            instance,
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
            instance: self.instance,
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
            instance: self.instance,
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
                Pin<Box<dyn Future<Output = Result<()>> + Send>>,
                Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>,
            >(Box::pin(async move { task.run(&mut accessor).await }))
        })));
        let handle = SpawnHandle(future.clone());
        (self.spawn)(future);
        handle
    }

    /// Retrieve the component instance of the caller.
    pub fn instance(&self) -> Instance {
        self.instance.unwrap()
    }

    #[doc(hidden)]
    pub fn maybe_instance(&self) -> Option<Instance> {
        self.instance
    }
}

type SpawnedFuture = Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;

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
pub trait AccessorTask<T, U, R>: Send + 'static {
    /// Run the task.
    fn run(self, accessor: &mut Accessor<T, U>) -> impl Future<Output = R> + Send;
}

struct State {
    host: *mut u8,
    store: *mut u8,
    spawned: Vec<Spawned>,
}

thread_local! {
    #[cfg(feature = "component-model-async")]
    static STATE: RefCell<Option<State>> = RefCell::new(None);
}

struct ResetState(Option<State>);

impl Drop for ResetState {
    fn drop(&mut self) {
        STATE.with(|v| {
            *v.borrow_mut() = self.0.take();
        })
    }
}

fn get_host_and_store() -> (*mut u8, *mut u8) {
    STATE
        .with(|v| {
            v.borrow()
                .as_ref()
                .map(|State { host, store, .. }| (*host, *store))
        })
        .unwrap()
}

fn spawn_task(task: Spawned) {
    STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
}

fn poll_with_state<T, F: Future + ?Sized>(
    store: VMStoreRawPtr,
    instance: SendSyncPtr<ComponentInstance>,
    cx: &mut Context,
    future: Pin<&mut F>,
) -> Poll<F::Output> {
    let mut store_cx = unsafe { StoreContextMut::new(&mut *store.0.as_ptr().cast()) };

    let (result, spawned) = {
        let host = store_cx.data_mut();
        let old = STATE.with(|v| {
            v.replace(Some(State {
                host: (host as *mut T).cast(),
                store: store.0.as_ptr().cast(),
                spawned: Vec::new(),
            }))
        });
        let _reset = ResetState(old);
        (future.poll(cx), STATE.with(|v| v.take()).unwrap().spawned)
    };

    let instance_ref = unsafe { &mut *instance.as_ptr() };
    for spawned in spawned {
        instance_ref.spawn(future::poll_fn(move |cx| {
            let mut spawned = spawned.try_lock().unwrap();
            let inner = mem::replace(DerefMut::deref_mut(&mut spawned), SpawnedInner::Aborted);
            if let SpawnedInner::Unpolled(mut future) | SpawnedInner::Polled { mut future, .. } =
                inner
            {
                let result = poll_with_state::<T, _>(store, instance, cx, future.as_mut());
                *DerefMut::deref_mut(&mut spawned) = SpawnedInner::Polled {
                    future,
                    waker: cx.waker().clone(),
                };
                result
            } else {
                Poll::Ready(Ok(()))
            }
        }))
    }

    result
}

/// Represents the state of a waitable handle.
#[derive(Debug)]
enum WaitableState {
    /// Represents a host task handle.
    HostTask,
    /// Represents a guest task handle.
    GuestTask,
    /// Represents a stream handle.
    Stream(TypeStreamTableIndex, StreamFutureState),
    /// Represents a future handle.
    Future(TypeFutureTableIndex, StreamFutureState),
    /// Represents a waitable-set handle
    Set,
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

impl ComponentInstance {
    pub(crate) fn instance(&self) -> Option<Instance> {
        self.instance
    }

    fn instance_states(&mut self) -> &mut HashMap<RuntimeComponentInstanceIndex, InstanceState> {
        &mut self.concurrent_state.instance_states
    }

    fn unblocked(&mut self) -> &mut HashSet<RuntimeComponentInstanceIndex> {
        &mut self.concurrent_state.unblocked
    }

    fn guest_task(&mut self) -> &mut Option<TableId<GuestTask>> {
        &mut self.concurrent_state.guest_task
    }

    fn push<V: Send + Sync + 'static>(&mut self, value: V) -> Result<TableId<V>, TableError> {
        self.concurrent_state.table.push(value)
    }

    fn get<V: 'static>(&self, id: TableId<V>) -> Result<&V, TableError> {
        self.concurrent_state.table.get(id)
    }

    fn get_mut<V: 'static>(&mut self, id: TableId<V>) -> Result<&mut V, TableError> {
        self.concurrent_state.table.get_mut(id)
    }

    pub fn add_child<T, U>(
        &mut self,
        child: TableId<T>,
        parent: TableId<U>,
    ) -> Result<(), TableError> {
        self.concurrent_state.table.add_child(child, parent)
    }

    pub fn remove_child<T, U>(
        &mut self,
        child: TableId<T>,
        parent: TableId<U>,
    ) -> Result<(), TableError> {
        self.concurrent_state.table.remove_child(child, parent)
    }

    fn delete<V: 'static>(&mut self, id: TableId<V>) -> Result<V, TableError> {
        self.concurrent_state.table.delete(id)
    }

    fn push_future(&mut self, future: HostTaskFuture) {
        self.concurrent_state
            .futures
            .get_mut()
            .unwrap()
            .get_mut()
            .push(future);
    }

    pub(crate) fn spawn(&mut self, task: impl Future<Output = Result<()>> + Send + 'static) {
        self.push_future(Box::pin(
            async move { HostTaskOutput::Background(task.await) },
        ))
    }

    fn yielding(&mut self) -> &mut HashMap<TableId<GuestTask>, Option<TableId<WaitableSet>>> {
        &mut self.concurrent_state.yielding
    }

    fn waitable_tables(
        &mut self,
    ) -> &mut PrimaryMap<RuntimeComponentInstanceIndex, StateTable<WaitableState>> {
        &mut self.concurrent_state.waitable_tables
    }

    fn error_context_tables(
        &mut self,
    ) -> &mut PrimaryMap<
        TypeComponentLocalErrorContextTableIndex,
        StateTable<LocalErrorContextRefCount>,
    > {
        &mut self.concurrent_state.error_context_tables
    }

    fn global_error_context_ref_counts(
        &mut self,
    ) -> &mut BTreeMap<TypeComponentGlobalErrorContextTableIndex, GlobalErrorContextRefCount> {
        &mut self.concurrent_state.global_error_context_ref_counts
    }

    fn maybe_push_call_context(&mut self, guest_task: TableId<GuestTask>) -> Result<()> {
        let task = self.get_mut(guest_task)?;
        if task.lift_result.is_some() {
            log::trace!("push call context for {}", guest_task.rep());
            let call_context = task.call_context.take().unwrap();
            unsafe { &mut (*self.store()) }
                .store_opaque_mut()
                .component_resource_state()
                .0
                .push(call_context);
        }
        Ok(())
    }

    fn maybe_pop_call_context(&mut self, guest_task: TableId<GuestTask>) -> Result<()> {
        if self.get_mut(guest_task)?.lift_result.is_some() {
            log::trace!("pop call context for {}", guest_task.rep());
            let call_context = Some(
                unsafe { &mut (*self.store()) }
                    .component_resource_state()
                    .0
                    .pop()
                    .unwrap(),
            );
            self.get_mut(guest_task)?.call_context = call_context;
        }
        Ok(())
    }

    fn may_enter(
        &mut self,
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
            match &self.get_mut(guest_task).unwrap().caller {
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

    fn maybe_send_event(
        &mut self,
        guest_task: TableId<GuestTask>,
        event: Event,
        waitable: Option<Waitable>,
    ) -> Result<()> {
        assert_ne!(Some(guest_task.rep()), waitable.map(|v| v.rep()));

        // We can only send an event to a caller if the callee has suspended at
        // least once; otherwise, we will not have had a chance to return a
        // `waitable` handle to the caller yet, in which case we can only queue the
        // event for later.
        let may_send = waitable
            .map(|v| v.has_suspended(self))
            .unwrap_or(Ok(true))?;

        if let (true, Some(callback)) = (may_send, self.get(guest_task)?.callback) {
            let handle = if let Some(waitable) = waitable {
                let Some((
                    handle,
                    WaitableState::HostTask
                    | WaitableState::GuestTask
                    | WaitableState::Stream(..)
                    | WaitableState::Future(..),
                )) = self.waitable_tables()[callback.instance].get_mut_by_rep(waitable.rep())
                else {
                    // If there's no handle found for the subtask, that means either the
                    // subtask was closed by the caller already or the caller called the
                    // callee via a sync-lowered import.  Either way, we can skip this
                    // event.
                    if let Event::CallReturned = event {
                        // Since this is a `CallReturned` event which will never be
                        // delivered to the caller, we must handle deleting the subtask
                        // here.
                        waitable.delete_from(self)?;
                    }
                    return Ok(());
                };
                handle
            } else {
                0
            };

            log::trace!(
                "use callback to deliver event {event:?} to {} for {:?} (handle {handle}): {:?}",
                guest_task.rep(),
                waitable.map(|v| v.rep()),
                callback.function,
            );

            if let Some(waitable) = waitable {
                waitable.on_delivery(self, event);
            }

            let old_task = self.guest_task().replace(guest_task);
            log::trace!(
                "maybe_send_event (callback): replaced {:?} with {} as current task",
                old_task.map(|v| v.rep()),
                guest_task.rep()
            );

            self.maybe_push_call_context(guest_task)?;

            let code =
                (callback.caller)(self, callback.instance, callback.function, event, handle)?;

            self.maybe_pop_call_context(guest_task)?;

            self.handle_callback_code(callback.instance, guest_task, code, Event::None)?;

            *self.guest_task() = old_task;
            log::trace!(
                "maybe_send_event (callback): restored {:?} as current task",
                old_task.map(|v| v.rep())
            );
        } else if let Some(waitable) = waitable {
            waitable.common(self)?.event = Some(event);
            waitable.mark_ready(self)?;

            let resumed = if let Event::CallReturned = event {
                if let Some((fiber, async_)) = self.get_mut(guest_task)?.deferred.take_stackful() {
                    log::trace!(
                        "use fiber to deliver event {event:?} to {} for {}",
                        guest_task.rep(),
                        waitable.rep()
                    );
                    let old_task = self.guest_task().replace(guest_task);
                    log::trace!(
                        "maybe_send_event (fiber): replaced {:?} with {} as current task",
                        old_task.map(|v| v.rep()),
                        guest_task.rep()
                    );
                    self.resume_stackful(guest_task, fiber, async_)?;
                    *self.guest_task() = old_task;
                    log::trace!(
                        "maybe_send_event (fiber): restored {:?} as current task",
                        old_task.map(|v| v.rep())
                    );
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
                    waitable.rep()
                );
            }
        } else {
            unreachable!("can't send waitable-less event to non-callback task");
        }

        Ok(())
    }

    fn resume_stackful(
        &mut self,
        guest_task: TableId<GuestTask>,
        mut fiber: StoreFiber<'static>,
        async_: bool,
    ) -> Result<()> {
        self.maybe_push_call_context(guest_task)?;

        let async_cx = AsyncCx::new(unsafe { (*self.store()).store_opaque_mut() });

        let mut store = Some(self.store());
        loop {
            break match resume_fiber(&mut fiber, store.take(), Ok(()))? {
                Ok((_, result)) => {
                    result?;
                    if async_ {
                        if self.get(guest_task)?.lift_result.is_some() {
                            return Err(anyhow!(crate::Trap::NoAsyncResult));
                        }
                    }
                    if let Some(instance) = fiber.instance {
                        self.maybe_resume_next_task(instance)?;
                        match &self.get(guest_task)?.caller {
                            Caller::Host(_) => {
                                log::trace!(
                                    "resume_stackful will delete task {}",
                                    guest_task.rep()
                                );
                                Waitable::Guest(guest_task).delete_from(self)?;
                            }
                            Caller::Guest { .. } => {
                                let waitable = Waitable::Guest(guest_task);
                                waitable.common(self)?.event = Some(Event::CallReturned);
                                waitable.send_or_mark_ready(self)?;
                            }
                        }
                    }
                    Ok(())
                }
                Err(new_store) => {
                    if new_store.is_some() {
                        self.maybe_pop_call_context(guest_task)?;
                        self.get_mut(guest_task)?.deferred = Deferred::Stackful { fiber, async_ };
                        Ok(())
                    } else {
                        // In this case, the fiber suspended while holding on to its
                        // `*mut dyn VMStore` instead of returning it to us.  That
                        // means we can't do anything else with the store (or self)
                        // for now; the only thing we can do is suspend _our_ fiber
                        // back up to the top level executor, which will resume us
                        // when we can make more progress, at which point we'll loop
                        // around and restore the child fiber again.
                        unsafe { async_cx.suspend(None) }?;
                        continue;
                    }
                }
            };
        }
    }

    fn handle_callback_code(
        &mut self,
        runtime_instance: RuntimeComponentInstanceIndex,
        guest_task: TableId<GuestTask>,
        code: u32,
        pending_event: Event,
    ) -> Result<()> {
        let (code, set) = unpack_callback_code(code);

        log::trace!(
            "received callback code from {}: {code} (set: {set})",
            guest_task.rep()
        );

        let task = self.get_mut(guest_task)?;
        let event = if task.lift_result.is_some() {
            if code == callback_code::EXIT {
                return Err(anyhow!(crate::Trap::NoAsyncResult));
            }
            pending_event
        } else {
            Event::CallReturned
        };

        let get_set = |instance: &mut Self, handle| {
            if handle == 0 {
                bail!("invalid waitable-set handle");
            }

            let (set, WaitableState::Set) =
                instance.waitable_tables()[runtime_instance].get_mut_by_index(handle)?
            else {
                bail!("invalid waitable-set handle");
            };

            Ok(TableId::<WaitableSet>::new(set))
        };

        if !matches!(event, Event::None) {
            let waitable = Waitable::Guest(guest_task);
            waitable.common(self)?.event = Some(event);
            waitable.send_or_mark_ready(self)?;
        }

        match code {
            callback_code::EXIT => match &self.get(guest_task)?.caller {
                Caller::Host(_) => {
                    log::trace!("handle_callback_code will delete task {}", guest_task.rep());
                    Waitable::Guest(guest_task).delete_from(self)?;
                }
                Caller::Guest { .. } => {
                    self.get_mut(guest_task)?.callback = None;
                }
            },
            callback_code::YIELD => {
                self.yielding().insert(guest_task, None);
            }
            callback_code::WAIT => {
                let set = get_set(self, set)?;
                let set = self.get_mut(set)?;

                if let Some(waitable) = set.ready.pop_first() {
                    let event = waitable.common(self)?.event.take().unwrap();
                    self.maybe_send_event(guest_task, event, Some(waitable))?;
                } else {
                    set.waiting.insert(guest_task);
                }
            }
            callback_code::POLL => {
                let set = get_set(self, set)?;
                self.get(set)?; // Just to make sure it exists
                self.yielding().insert(guest_task, Some(set));
            }
            _ => bail!("unsupported callback code: {code}"),
        }

        Ok(())
    }

    fn resume_stackless(
        &mut self,
        guest_task: TableId<GuestTask>,
        call: Box<dyn FnOnce(&mut ComponentInstance) -> Result<u32>>,
        runtime_instance: RuntimeComponentInstanceIndex,
    ) -> Result<()> {
        self.maybe_push_call_context(guest_task)?;

        let code = call(self)?;

        self.maybe_pop_call_context(guest_task)?;

        self.handle_callback_code(runtime_instance, guest_task, code, Event::CallStarted)?;

        self.maybe_resume_next_task(runtime_instance)
    }

    fn poll_for_result(&mut self) -> Result<()> {
        let task = *self.guest_task();
        self.poll_loop(move |instance| {
            task.map(|task| Ok::<_, anyhow::Error>(instance.get(task)?.result.is_none()))
                .unwrap_or(Ok(true))
        })
    }

    fn handle_ready(&mut self, ready: Vec<HostTaskOutput>) -> Result<()> {
        for output in ready {
            match output {
                HostTaskOutput::Background(result) => {
                    result?;
                }
                HostTaskOutput::Waitable { waitable, fun } => {
                    let waitable = waitable.load(Relaxed);
                    if waitable != 0 {
                        let event = fun(self)?;
                        log::trace!("handle_ready event {event:?} for {waitable}");
                        let waitable = match event {
                            Event::CallReturned => {
                                Waitable::Host(TableId::<HostTask>::new(waitable))
                            }
                            Event::StreamRead { .. }
                            | Event::FutureRead { .. }
                            | Event::StreamWrite { .. }
                            | Event::FutureWrite { .. } => {
                                Waitable::Transmit(TableId::<TransmitHandle>::new(waitable))
                            }
                            _ => unreachable!(),
                        };
                        waitable.common(self)?.event = Some(event);
                        waitable.send_or_mark_ready(self)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn maybe_yield(&mut self) -> Result<()> {
        let guest_task = self.guest_task().unwrap();

        if self.get(guest_task)?.should_yield {
            log::trace!("maybe_yield suspend {}", guest_task.rep());

            self.yielding().insert(guest_task, None);
            unsafe {
                let cx = AsyncCx::new((*self.store()).store_opaque_mut());
                cx.suspend(Some(self.store()))
            }?
            .unwrap();

            log::trace!("maybe_yield resume {}", guest_task.rep());
        } else {
            log::trace!("maybe_yield skip {}", guest_task.rep());
        }

        Ok(())
    }

    fn unyield(&mut self) -> Result<bool> {
        let mut resumed = false;
        for (guest_task, set) in mem::take(self.yielding()) {
            if self.get(guest_task)?.callback.is_some() {
                resumed = true;

                let (waitable, event) = if let Some(set) = set {
                    if let Some(waitable) = self.get_mut(set)?.ready.pop_first() {
                        waitable
                            .common(self)?
                            .event
                            .take()
                            .map(|e| (Some(waitable), e))
                    } else {
                        None
                    }
                } else {
                    None
                }
                .unwrap_or((None, Event::None));

                self.maybe_send_event(guest_task, event, waitable)?;
            } else {
                assert!(set.is_none());

                if let Some((fiber, async_)) = self
                    .get_mut(guest_task)
                    .with_context(|| format!("guest task {}", guest_task.rep()))?
                    .deferred
                    .take_stackful()
                {
                    resumed = true;
                    let old_task = self.guest_task().replace(guest_task);
                    log::trace!(
                        "unyield: replaced {:?} with {} as current task",
                        old_task.map(|v| v.rep()),
                        guest_task.rep()
                    );
                    self.resume_stackful(guest_task, fiber, async_)?;
                    *self.guest_task() = old_task;
                    log::trace!(
                        "unyield: restored {:?} as current task",
                        old_task.map(|v| v.rep())
                    );
                }
            }
        }

        for instance in mem::take(self.unblocked()) {
            let entry = self.instance_states().entry(instance).or_default();

            if !(entry.backpressure || entry.in_sync_call) {
                if let Some(task) = entry.task_queue.pop_front() {
                    resumed = true;
                    self.resume(task)?;
                }
            }
        }

        Ok(resumed)
    }

    fn poll_loop(&mut self, mut continue_: impl FnMut(&mut Self) -> Result<bool>) -> Result<()> {
        loop {
            let ready = {
                let store = self.store();
                let futures = self.concurrent_state.futures.get_mut().unwrap();
                let mut future = pin!(futures.next());
                unsafe {
                    let cx = AsyncCx::new((*store).store_opaque_mut());
                    cx.poll(future.as_mut())
                }
            };

            match ready {
                Poll::Ready(Some(ready)) => {
                    self.handle_ready(ready)?;
                }
                Poll::Ready(None) => {
                    let resumed = self.unyield()?;
                    if !resumed {
                        log::trace!("exhausted future queue; exiting poll_loop");
                        break;
                    }
                }
                Poll::Pending => {
                    let resumed = self.unyield()?;
                    if continue_(self)? {
                        unsafe {
                            let cx = AsyncCx::new((*self.store()).store_opaque_mut());
                            cx.suspend(Some(self.store()))
                        }?
                        .unwrap();
                    } else if !resumed {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn resume(&mut self, task: TableId<GuestTask>) -> Result<()> {
        log::trace!("resume {}", task.rep());

        // TODO: Avoid calling `resume_stackful` or `resume_stackless` here, because it may call us, leading to
        // recursion limited only by the number of waiters.  Flatten this into an iteration instead.
        let old_task = self.guest_task().replace(task);
        log::trace!(
            "resume: replaced {:?} with {} as current task",
            old_task.map(|v| v.rep()),
            task.rep()
        );
        match mem::replace(&mut self.get_mut(task)?.deferred, Deferred::None) {
            Deferred::None => unreachable!(),
            Deferred::Stackful { fiber, async_ } => self.resume_stackful(task, fiber, async_),
            Deferred::Stackless { call, instance } => self.resume_stackless(task, call, instance),
        }?;
        *self.guest_task() = old_task;
        log::trace!(
            "resume: restored {:?} as current task",
            old_task.map(|v| v.rep())
        );
        Ok(())
    }

    fn maybe_resume_next_task(&mut self, instance: RuntimeComponentInstanceIndex) -> Result<()> {
        let state = self.instance_states().get_mut(&instance).unwrap();

        if state.backpressure || state.in_sync_call {
            Ok(())
        } else {
            if let Some(next) = state.task_queue.pop_front() {
                self.resume(next)
            } else {
                Ok(())
            }
        }
    }

    fn loop_until<R>(
        &mut self,
        mut future: Pin<Box<dyn Future<Output = R> + Send + '_>>,
    ) -> Result<R> {
        let result = Arc::new(Mutex::new(None));
        let poll = &mut {
            let result = result.clone();
            move |instance: &mut ComponentInstance| {
                let ready = unsafe {
                    let cx = AsyncCx::new((*instance.store()).store_opaque_mut());
                    cx.poll(future.as_mut())
                };
                Ok(match ready {
                    Poll::Ready(value) => {
                        *result.lock().unwrap() = Some(value);
                        false
                    }
                    Poll::Pending => true,
                })
            }
        };
        self.poll_loop(|instance| poll(instance))?;

        // Poll once more to get the result if necessary:
        if result.lock().unwrap().is_none() {
            poll(self)?;
        }

        let result = result
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow!(crate::Trap::NoAsyncResult));

        result
    }

    fn do_start_call<T>(
        &mut self,
        mut store: StoreContextMut<T>,
        guest_task: TableId<GuestTask>,
        async_: bool,
        call: impl FnOnce(
                StoreContextMut<T>,
                &mut ComponentInstance,
            ) -> Result<[MaybeUninit<ValRaw>; MAX_FLAT_PARAMS]>
            + Send
            + Sync
            + 'static,
        callback: Option<SendSyncPtr<VMFuncRef>>,
        post_return: Option<SendSyncPtr<VMFuncRef>>,
        callee_instance: RuntimeComponentInstanceIndex,
        result_count: usize,
        use_fiber: bool,
    ) -> Result<()> {
        let state = &mut self.instance_states().entry(callee_instance).or_default();
        let ready = state.task_queue.is_empty() && !(state.backpressure || state.in_sync_call);

        let mut code = callback_code::EXIT;
        let mut async_finished = false;

        if callback.is_some() {
            assert!(async_);

            if ready {
                self.maybe_push_call_context(guest_task)?;
                let storage = call(store.as_context_mut(), self)?;
                code = unsafe { storage[0].assume_init() }.get_i32() as u32;
                async_finished = code == callback_code::EXIT;
                self.maybe_pop_call_context(guest_task)?;
            } else {
                self.instance_states()
                    .get_mut(&callee_instance)
                    .unwrap()
                    .task_queue
                    .push_back(guest_task);

                self.get_mut(guest_task)?.deferred = Deferred::Stackless {
                    call: Box::new(move |instance| {
                        let mut store = unsafe { StoreContextMut(&mut *instance.store().cast()) };
                        let old_task = instance.guest_task().replace(guest_task);
                        log::trace!(
                            "do_start_call: replaced {:?} with {} as current task",
                            old_task.map(|v| v.rep()),
                            guest_task.rep()
                        );
                        let storage = call(store.as_context_mut(), instance)?;
                        *instance.guest_task() = old_task;
                        log::trace!(
                            "do_start_call: restored {:?} as current task",
                            old_task.map(|v| v.rep())
                        );
                        Ok(unsafe { storage[0].assume_init() }.get_i32() as u32)
                    }),
                    instance: callee_instance,
                };
            }
        } else {
            let do_call = move |mut store: StoreContextMut<T>, instance: &mut ComponentInstance| {
                let mut flags = instance.instance_flags(callee_instance);

                if !async_ {
                    instance
                        .instance_states()
                        .get_mut(&callee_instance)
                        .unwrap()
                        .in_sync_call = true;
                }

                let storage = call(store.as_context_mut(), instance)?;

                if !async_ {
                    instance
                        .instance_states()
                        .get_mut(&callee_instance)
                        .unwrap()
                        .in_sync_call = false;

                    let lift = instance.get_mut(guest_task)?.lift_result.take().unwrap();

                    assert!(instance.get(guest_task)?.result.is_none());

                    let result = (lift.lift)(instance, unsafe {
                        mem::transmute::<&[MaybeUninit<ValRaw>], &[ValRaw]>(
                            &storage[..result_count],
                        )
                    })?;

                    unsafe { flags.set_needs_post_return(false) }

                    if let Some(func) = post_return {
                        let arg = match result_count {
                            0 => ValRaw::i32(0),
                            1 => unsafe { storage[0].assume_init() },
                            _ => unreachable!(),
                        };
                        unsafe {
                            crate::Func::call_unchecked_raw(
                                &mut store,
                                func.as_non_null(),
                                NonNull::new(ptr::slice_from_raw_parts(&arg, 1).cast_mut())
                                    .unwrap(),
                            )?;
                        }
                    }

                    unsafe { flags.set_may_enter(true) }

                    let (calls, host_table, _) = store.0.component_resource_state();
                    ResourceTables {
                        calls,
                        host_table: Some(host_table),
                        tables: Some(instance.component_resource_tables()),
                    }
                    .exit_call()?;

                    if let Caller::Host(tx) = &mut instance.get_mut(guest_task)?.caller {
                        if let Some(tx) = tx.take() {
                            _ = tx.send(result);
                        }
                    } else {
                        instance.get_mut(guest_task)?.result = Some(result);
                    }
                }

                Ok(())
            };

            if use_fiber {
                let mut fiber = unsafe {
                    make_fiber(store.traitobj().as_ptr(), Some(callee_instance), {
                        let instance = self as *mut ComponentInstance;
                        move |store| {
                            let store = StoreContextMut::<T>(&mut *store.cast());
                            let instance = &mut *instance;
                            do_call(store, instance)
                        }
                    })
                }?;

                self.get_mut(guest_task)?.should_yield = true;

                if ready {
                    self.maybe_push_call_context(guest_task)?;
                    let mut store = Some(store.traitobj().as_ptr());
                    loop {
                        match resume_fiber(&mut fiber, store.take(), Ok(()))? {
                            Ok((_, result)) => {
                                result?;
                                async_finished = async_;
                                self.maybe_resume_next_task(callee_instance)?;
                                break;
                            }
                            Err(store) => {
                                if store.is_some() {
                                    self.maybe_pop_call_context(guest_task)?;
                                    self.get_mut(guest_task)?.deferred =
                                        Deferred::Stackful { fiber, async_ };
                                    break;
                                } else {
                                    unsafe {
                                        suspend_fiber(fiber.suspend, fiber.stack_limit, None)?;
                                    };
                                }
                            }
                        }
                    }
                } else {
                    self.instance_states()
                        .get_mut(&callee_instance)
                        .unwrap()
                        .task_queue
                        .push_back(guest_task);

                    self.get_mut(guest_task)?.deferred = Deferred::Stackful { fiber, async_ };
                }
            } else {
                self.maybe_push_call_context(guest_task)?;
                do_call(store.as_context_mut(), self)?;
                async_finished = async_;
                self.maybe_pop_call_context(guest_task)?;
            }
        };

        let guest_task = self.guest_task().take().unwrap();

        let caller = if let Caller::Guest { task, .. } = &self.get(guest_task)?.caller {
            Some(*task)
        } else {
            None
        };
        *self.guest_task() = caller;
        log::trace!(
            "popped current task {}; new task is {:?}",
            guest_task.rep(),
            caller.map(|v| v.rep())
        );

        let task = self.get_mut(guest_task)?;

        if code == callback_code::EXIT
            && async_finished
            && !(matches!(&task.caller, Caller::Guest {..} if task.result.is_some())
                || matches!(&task.caller, Caller::Host(tx) if tx.is_none()))
        {
            Err(anyhow!(crate::Trap::NoAsyncResult))
        } else {
            if ready && callback.is_some() {
                self.handle_callback_code(callee_instance, guest_task, code, Event::CallStarted)?;
            }

            Ok(())
        }
    }

    fn enter_call<T>(
        &mut self,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        memory: *mut VMMemoryDefinition,
        string_encoding: u8,
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

        let sync_caller = matches!(caller_info, CallerInfo::Sync { .. });

        let start = SendSyncPtr::new(NonNull::new(start).unwrap());
        let return_ = SendSyncPtr::new(NonNull::new(return_).unwrap());
        let old_task = self.guest_task().take();
        let old_task_rep = old_task.map(|v| v.rep());
        let new_task = GuestTask::new(
            self,
            Box::new(move |instance, dst| {
                let mut store = unsafe { StoreContextMut::<T>(&mut *instance.store().cast()) };
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
                        &mut store,
                        start.as_non_null(),
                        NonNull::new(
                            &mut src[..count.max(dst.len())] as *mut [MaybeUninit<ValRaw>] as _,
                        )
                        .unwrap(),
                    )?;
                }
                dst.copy_from_slice(&src[..dst.len()]);
                let task = instance.guest_task().unwrap();
                if old_task_rep.is_some() {
                    let waitable = Waitable::Guest(task);
                    waitable.common(instance)?.event = Some(Event::CallStarted);
                    waitable.send_or_mark_ready(instance)?;
                }
                Ok(())
            }),
            LiftResult {
                lift: Box::new(move |instance, src| {
                    let mut store = unsafe { StoreContextMut::<T>(&mut *instance.store().cast()) };
                    let mut my_src = src.to_owned(); // TODO: use stack to avoid allocation?
                    if let ResultInfo::Heap { results } = &result_info {
                        my_src.push(ValRaw::u32(*results));
                    }
                    unsafe {
                        crate::Func::call_unchecked_raw(
                            &mut store,
                            return_.as_non_null(),
                            my_src.as_mut_slice().into(),
                        )?;
                    }
                    let task = instance.guest_task().unwrap();
                    if sync_caller {
                        instance.get_mut(task)?.sync_result =
                            Some(if let ResultInfo::Stack { result_count } = &result_info {
                                match result_count {
                                    0 => None,
                                    1 => Some(my_src[0]),
                                    _ => unreachable!(),
                                }
                            } else {
                                None
                            });
                    }
                    if old_task_rep.is_some() {
                        let waitable = Waitable::Guest(task);
                        waitable.common(instance)?.event = Some(Event::CallReturned);
                        waitable.send_or_mark_ready(instance)?;
                    }
                    Ok(Box::new(DummyResult) as Box<dyn std::any::Any + Send + Sync>)
                }),
                ty: task_return_type,
                memory: NonNull::new(memory).map(SendSyncPtr::new),
                string_encoding: StringEncoding::from_u8(string_encoding).unwrap(),
            },
            Caller::Guest {
                task: old_task.unwrap(),
                instance: caller_instance,
            },
            None,
        )?;

        let guest_task = self.push(new_task)?;
        self.get_mut(guest_task)?
            .common
            .rep
            .store(guest_task.rep(), Relaxed);

        if let Some(old_task) = old_task {
            self.get_mut(old_task)?.subtasks.insert(guest_task);
        };

        *self.guest_task() = Some(guest_task);
        log::trace!(
            "pushed {} as current task; old task was {:?}",
            guest_task.rep(),
            old_task.map(|v| v.rep())
        );

        Ok(())
    }

    fn call_callback<T>(
        &mut self,
        callee_instance: RuntimeComponentInstanceIndex,
        function: SendSyncPtr<VMFuncRef>,
        event: Event,
        handle: u32,
    ) -> Result<u32> {
        let mut store = unsafe { StoreContextMut::<T>(&mut *self.store().cast()) };
        let mut flags = self.instance_flags(callee_instance);

        let (ordinal, result) = event.parts();
        let params = &mut [
            ValRaw::u32(ordinal),
            ValRaw::u32(handle),
            ValRaw::u32(result),
        ];
        unsafe {
            flags.set_may_enter(false);
            crate::Func::call_unchecked_raw(
                &mut store,
                function.as_non_null(),
                params.as_mut_slice().into(),
            )?;
            flags.set_may_enter(true);
        }
        Ok(params[0].get_u32())
    }

    fn exit_call<T>(
        &mut self,
        mut store: StoreContextMut<T>,
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
        let guest_task = self.guest_task().unwrap();
        let callee = SendSyncPtr::new(NonNull::new(callee).unwrap());
        let param_count = usize::try_from(param_count).unwrap();
        assert!(param_count <= MAX_FLAT_PARAMS);
        let result_count = usize::try_from(result_count).unwrap();
        assert!(result_count <= MAX_FLAT_RESULTS);

        if !callback.is_null() {
            self.get_mut(guest_task)?.callback = Some(Callback {
                function: SendSyncPtr::new(NonNull::new(callback).unwrap()),
                caller: Self::call_callback::<T>,
                instance: callee_instance,
            });
        }

        let call = make_call(
            guest_task,
            callee,
            callee_instance,
            param_count,
            result_count,
            if callback.is_null() {
                None
            } else {
                Some(self.instance_flags(callee_instance))
            },
        );

        self.do_start_call(
            store.as_context_mut(),
            guest_task,
            (flags & EXIT_FLAG_ASYNC_CALLEE) != 0,
            call,
            NonNull::new(callback).map(SendSyncPtr::new),
            NonNull::new(post_return).map(SendSyncPtr::new),
            callee_instance,
            result_count,
            true,
        )?;

        let task = self.get(guest_task)?;

        let mut status = if task.lower_params.is_some() {
            Status::Starting
        } else if task.lift_result.is_some() {
            Status::Started
        } else {
            Status::Returned
        };

        log::trace!("status {status:?} for {}", guest_task.rep());

        let call = if status != Status::Returned {
            if async_caller {
                self.get_mut(guest_task)?.has_suspended = true;

                self.waitable_tables()[caller_instance]
                    .insert(guest_task.rep(), WaitableState::GuestTask)?
            } else {
                let caller = if let Caller::Guest { task, .. } = &task.caller {
                    *task
                } else {
                    unreachable!()
                };

                let set = self.get_mut(caller)?.sync_call_set;
                self.get_mut(set)?.waiting.insert(caller);
                Waitable::Guest(guest_task).join(self, Some(set))?;

                self.poll_for_result()?;
                status = Status::Returned;
                0
            }
        } else {
            0
        };

        if let Some(storage) = storage {
            if let Some(result) = self.get_mut(guest_task)?.sync_result.take() {
                if let Some(result) = result {
                    storage[0] = MaybeUninit::new(result);
                }
            } else {
                return Err(anyhow!(crate::Trap::NoAsyncResult));
            }
            Ok(0)
        } else {
            Ok(((status as u32) << 30) | call)
        }
    }

    pub(crate) fn wrap_call<T, F, P, R>(
        &mut self,
        store: StoreContextMut<T>,
        closure: Arc<F>,
        params: P,
    ) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'static>>
    where
        F: for<'a> Fn(
                &'a mut Accessor<T, T>,
                P,
            ) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'a>>
            + Send
            + Sync
            + 'static,
        P: Lift + Send + Sync + 'static,
        R: Lower + Send + Sync + 'static,
    {
        let mut accessor =
            unsafe { Accessor::new(get_host_and_store, spawn_task, self.instance()) };
        let mut future = Box::pin(async move { closure(&mut accessor, params).await });
        let store = VMStoreRawPtr(store.traitobj());
        let instance = SendSyncPtr::new(NonNull::new(self).unwrap());
        let future = future::poll_fn(move |cx| {
            poll_with_state::<T, _>(store, instance, cx, future.as_mut())
        });

        unsafe {
            mem::transmute::<
                Pin<Box<dyn Future<Output = Result<R>> + Send>>,
                Pin<Box<dyn Future<Output = Result<R>> + Send + 'static>>,
            >(Box::pin(future))
        }
    }

    pub(crate) fn wrap_dynamic_call<T, F>(
        &mut self,
        store: StoreContextMut<T>,
        closure: Arc<F>,
        params: Vec<Val>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Val>>> + Send + 'static>>
    where
        F: for<'a> Fn(
                &'a mut Accessor<T, T>,
                Vec<Val>,
            ) -> Pin<Box<dyn Future<Output = Result<Vec<Val>>> + Send + 'a>>
            + Send
            + Sync
            + 'static,
    {
        let mut accessor =
            unsafe { Accessor::new(get_host_and_store, spawn_task, self.instance()) };
        let mut future = Box::pin(async move { closure(&mut accessor, params).await });
        let store = VMStoreRawPtr(store.traitobj());
        let instance = SendSyncPtr::new(NonNull::new(self).unwrap());
        let future = future::poll_fn(move |cx| {
            poll_with_state::<T, _>(store, instance, cx, future.as_mut())
        });

        unsafe {
            mem::transmute::<
                Pin<Box<dyn Future<Output = Result<Vec<Val>>> + Send>>,
                Pin<Box<dyn Future<Output = Result<Vec<Val>>> + Send + 'static>>,
            >(Box::pin(future))
        }
    }

    pub(crate) fn first_poll<T, R: Send + Sync + 'static>(
        &mut self,
        store: StoreContextMut<T>,
        future: impl Future<Output = Result<R>> + Send + 'static,
        caller_instance: RuntimeComponentInstanceIndex,
        lower: impl FnOnce(StoreContextMut<T>, R) -> Result<()> + Send + 'static,
    ) -> Result<Option<u32>> {
        let caller = self.guest_task().unwrap();
        let task = self.push(HostTask::new(caller_instance))?;
        let waitable = self.get(task)?.common.rep.clone();
        waitable.store(task.rep(), Relaxed);

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

        log::trace!("new host task child of {}: {}", caller.rep(), task.rep());
        let mut future = Box::pin(future.map(move |result| HostTaskOutput::Waitable {
            waitable,
            fun: Box::new(move |instance| {
                let store = unsafe { StoreContextMut(&mut *instance.store().cast()) };
                lower(store, result?)?;
                Ok(Event::CallReturned)
            }),
        })) as HostTaskFuture;

        Ok(
            match future
                .as_mut()
                .poll(&mut Context::from_waker(&dummy_waker()))
            {
                Poll::Ready(output) => {
                    let HostTaskOutput::Waitable { fun, .. } = output else {
                        unreachable!()
                    };
                    log::trace!("delete host task {} (already ready)", task.rep());
                    self.delete(task)?;
                    fun(self)?;
                    None
                }
                Poll::Pending => {
                    self.get_mut(task)?.has_suspended = true;
                    self.push_future(future);
                    Some(
                        self.waitable_tables()[caller_instance]
                            .insert(task.rep(), WaitableState::HostTask)?,
                    )
                }
            },
        )
    }

    pub(crate) fn poll_and_block<T, R: Send + Sync + 'static>(
        &mut self,
        mut store: StoreContextMut<T>,
        future: impl Future<Output = Result<R>> + Send + 'static,
        caller_instance: RuntimeComponentInstanceIndex,
    ) -> Result<R> {
        let Some(caller) = *self.guest_task() else {
            return match pin!(future).poll(&mut Context::from_waker(&dummy_waker())) {
                Poll::Ready(result) => result,
                Poll::Pending => {
                    unreachable!()
                }
            };
        };
        let old_result = self
            .get_mut(caller)
            .with_context(|| format!("bad handle: {}", caller.rep()))?
            .result
            .take();
        let task = self.push(HostTask::new(caller_instance))?;
        let waitable = self.get(task)?.common.rep.clone();
        waitable.store(task.rep(), Relaxed);

        log::trace!("new host task child of {}: {}", caller.rep(), task.rep());
        let mut future = Box::pin(future.map(move |result| HostTaskOutput::Waitable {
            waitable,
            fun: Box::new(move |instance| {
                instance.get_mut(caller)?.result = Some(Box::new(result?) as _);
                Ok(Event::CallReturned)
            }),
        })) as HostTaskFuture;

        let Some(cx) = AsyncCx::try_new(&mut store.0) else {
            return Err(anyhow!("future dropped"));
        };

        Ok(match unsafe { cx.poll(future.as_mut()) } {
            Poll::Ready(output) => {
                let HostTaskOutput::Waitable { fun, .. } = output else {
                    unreachable!()
                };
                log::trace!("delete host task {} (already ready)", task.rep());
                self.delete(task)?;
                fun(self)?;
                let result = *mem::replace(&mut self.get_mut(caller)?.result, old_result)
                    .unwrap()
                    .downcast()
                    .unwrap();
                result
            }
            Poll::Pending => {
                self.push_future(future);

                let set = self.get_mut(caller)?.sync_call_set;
                self.get_mut(set)?.waiting.insert(caller);
                Waitable::Host(task).join(self, Some(set))?;

                self.poll_loop(|instance| Ok(instance.get_mut(caller)?.result.is_none()))?;

                if let Some(result) = self.get_mut(caller)?.result.take() {
                    self.get_mut(caller)?.result = old_result;
                    *result.downcast().unwrap()
                } else {
                    return Err(anyhow!(crate::Trap::NoAsyncResult));
                }
            }
        })
    }

    async fn poll_until<T: Send, R: Send + Sync + 'static>(
        &mut self,
        store: StoreContextMut<'_, T>,
        future: Pin<Box<dyn Future<Output = R> + Send + '_>>,
    ) -> Result<R> {
        unsafe {
            on_fiber_raw(VMStoreRawPtr(store.traitobj()), None, move |_| {
                self.loop_until(future)
            })
        }
        .await?
    }

    pub(crate) fn task_return(
        &mut self,
        ty: TypeTupleIndex,
        memory: *mut VMMemoryDefinition,
        string_encoding: u8,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()> {
        let storage = unsafe { std::slice::from_raw_parts(storage, storage_len) };
        let guest_task = self.guest_task().unwrap();
        let lift = self
            .get_mut(guest_task)?
            .lift_result
            .take()
            .ok_or_else(|| anyhow!("`task.return` called more than once for current task"))?;

        if ty != lift.ty
            || (!memory.is_null()
                && memory != lift.memory.map(|v| v.as_ptr()).unwrap_or(ptr::null_mut()))
            || string_encoding != lift.string_encoding as u8
        {
            bail!("invalid `task.return` signature and/or options for current task");
        }

        assert!(self.get(guest_task)?.result.is_none());

        log::trace!("task.return for {}", guest_task.rep());

        let result = (lift.lift)(self, storage)?;

        let (calls, host_table, _) = unsafe { &mut *self.store() }
            .store_opaque_mut()
            .component_resource_state();
        ResourceTables {
            calls,
            host_table: Some(host_table),
            tables: Some(self.component_resource_tables()),
        }
        .exit_call()?;

        if let Caller::Host(tx) = &mut self.get_mut(guest_task)?.caller {
            if let Some(tx) = tx.take() {
                _ = tx.send(result);
            }
        } else {
            self.get_mut(guest_task)?.result = Some(result);
        }

        Ok(())
    }

    pub(crate) fn backpressure_set(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        enabled: u32,
    ) -> Result<()> {
        let entry = self.instance_states().entry(caller_instance).or_default();
        let old = entry.backpressure;
        let new = enabled != 0;
        entry.backpressure = new;

        if old && !new && !entry.task_queue.is_empty() {
            self.unblocked().insert(caller_instance);
        }

        Ok(())
    }

    pub(crate) fn waitable_set_new(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
    ) -> Result<u32> {
        let set = self.push(WaitableSet::default())?;
        let handle =
            self.waitable_tables()[caller_instance].insert(set.rep(), WaitableState::Set)?;
        log::trace!("new waitable set {} (handle {handle})", set.rep());
        Ok(handle)
    }

    pub(crate) fn waitable_set_wait(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        memory: *mut VMMemoryDefinition,
        set: u32,
        payload: u32,
    ) -> Result<u32> {
        let (rep, WaitableState::Set) =
            self.waitable_tables()[caller_instance].get_mut_by_index(set)?
        else {
            bail!("invalid waitable-set handle");
        };

        self.waitable_check(
            async_,
            WaitableCheck::Wait(WaitableCheckParams {
                set: TableId::new(rep),
                memory,
                payload,
                caller_instance,
            }),
        )
    }

    pub(crate) fn waitable_set_poll(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        memory: *mut VMMemoryDefinition,
        set: u32,
        payload: u32,
    ) -> Result<u32> {
        let (rep, WaitableState::Set) =
            self.waitable_tables()[caller_instance].get_mut_by_index(set)?
        else {
            bail!("invalid waitable-set handle");
        };

        self.waitable_check(
            async_,
            WaitableCheck::Poll(WaitableCheckParams {
                set: TableId::new(rep),
                memory,
                payload,
                caller_instance,
            }),
        )
    }

    pub(crate) fn yield_(&mut self, async_: bool) -> Result<()> {
        self.waitable_check(async_, WaitableCheck::Yield).map(drop)
    }

    pub(crate) fn waitable_check(&mut self, async_: bool, check: WaitableCheck) -> Result<u32> {
        if async_ {
            bail!(
                "todo: async `waitable-set.wait`, `waitable-set.poll`, and `yield` not yet implemented"
            );
        }

        let guest_task = self.guest_task().unwrap();

        let (wait, set) = match &check {
            WaitableCheck::Wait(params) => {
                self.get_mut(params.set)?.waiting.insert(guest_task);
                (true, Some(params.set))
            }
            WaitableCheck::Poll(params) => (false, Some(params.set)),
            WaitableCheck::Yield => (false, None),
        };

        log::trace!(
            "waitable check for {}; set {:?}",
            guest_task.rep(),
            set.map(|v| v.rep())
        );

        if wait && self.get(guest_task)?.callback.is_some() {
            bail!("cannot call `task.wait` from async-lifted export with callback");
        }

        let set_is_empty = |instance: &mut Self| {
            Ok::<_, anyhow::Error>(
                set.map(|set| Ok::<_, anyhow::Error>(instance.get(set)?.ready.is_empty()))
                    .transpose()?
                    .unwrap_or(true),
            )
        };

        if matches!(check, WaitableCheck::Yield) || set_is_empty(self)? {
            self.maybe_yield()?;

            if set_is_empty(self)? {
                self.poll_loop(move |instance| {
                    Ok::<_, anyhow::Error>(wait && set_is_empty(instance)?)
                })?;
            }
        }

        log::trace!(
            "waitable check for {}; set {:?}, part two",
            guest_task.rep(),
            set.map(|v| v.rep())
        );

        let result = match check {
            WaitableCheck::Wait(params) => {
                self.get_mut(params.set)?.waiting.remove(&guest_task);

                let waitable = self
                    .get_mut(params.set)?
                    .ready
                    .pop_first()
                    .ok_or_else(|| anyhow!("no waitables to wait for"))?;

                let event = waitable.common(self)?.event.take().unwrap();

                log::trace!(
                    "deliver event {event:?} via waitable-set.wait to {} for {}; set {}",
                    guest_task.rep(),
                    waitable.rep(),
                    params.set.rep()
                );

                let entry =
                    self.waitable_tables()[params.caller_instance].get_mut_by_rep(waitable.rep());
                let Some((
                    handle,
                    WaitableState::HostTask
                    | WaitableState::GuestTask
                    | WaitableState::Stream(..)
                    | WaitableState::Future(..),
                )) = entry
                else {
                    bail!("handle not found for waitable rep {}", waitable.rep());
                };

                waitable.on_delivery(self, event);

                let (ordinal, result) = event.parts();
                let store = unsafe { (*self.store()).store_opaque_mut() };
                let options = unsafe {
                    Options::new(
                        store.id(),
                        NonNull::new(params.memory),
                        None,
                        StringEncoding::Utf8,
                        true,
                        None,
                    )
                };
                let ptr = func::validate_inbounds::<(u32, u32)>(
                    options.memory_mut(store),
                    &ValRaw::u32(params.payload),
                )?;
                options.memory_mut(store)[ptr + 0..][..4].copy_from_slice(&handle.to_le_bytes());
                options.memory_mut(store)[ptr + 4..][..4].copy_from_slice(&result.to_le_bytes());

                Ok(ordinal)
            }
            WaitableCheck::Poll(params) => {
                if let Some(waitable) = self.get_mut(params.set)?.ready.pop_first() {
                    let event = waitable.common(self)?.event.take().unwrap();

                    log::trace!(
                        "deliver event {event:?} via waitable-set.poll to {} for {}; set {}",
                        guest_task.rep(),
                        waitable.rep(),
                        params.set.rep()
                    );

                    let entry = self.waitable_tables()[params.caller_instance]
                        .get_mut_by_rep(waitable.rep());
                    let Some((
                        handle,
                        WaitableState::HostTask
                        | WaitableState::GuestTask
                        | WaitableState::Stream(..)
                        | WaitableState::Future(..),
                    )) = entry
                    else {
                        bail!("handle not found for waitable rep {}", waitable.rep());
                    };

                    waitable.on_delivery(self, event);

                    let (ordinal, result) = event.parts();
                    let store = unsafe { (*self.store()).store_opaque_mut() };
                    let options = unsafe {
                        Options::new(
                            store.id(),
                            NonNull::new(params.memory),
                            None,
                            StringEncoding::Utf8,
                            true,
                            None,
                        )
                    };
                    let ptr = func::validate_inbounds::<(u32, u32, u32)>(
                        options.memory_mut(store),
                        &ValRaw::u32(params.payload),
                    )?;
                    options.memory_mut(store)[ptr + 0..][..4]
                        .copy_from_slice(&ordinal.to_le_bytes());
                    options.memory_mut(store)[ptr + 4..][..4]
                        .copy_from_slice(&handle.to_le_bytes());
                    options.memory_mut(store)[ptr + 8..][..4]
                        .copy_from_slice(&result.to_le_bytes());

                    Ok(1)
                } else {
                    log::trace!(
                        "no events ready to deliver via waitable-set.poll to {}; set {}",
                        guest_task.rep(),
                        params.set.rep()
                    );

                    Ok(0)
                }
            }
            WaitableCheck::Yield => Ok(0),
        };

        result
    }

    pub(crate) fn waitable_set_drop(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        set: u32,
    ) -> Result<()> {
        let (rep, WaitableState::Set) =
            self.waitable_tables()[caller_instance].remove_by_index(set)?
        else {
            bail!("invalid waitable-set handle");
        };

        log::trace!("drop waitable set {rep} (handle {set})");

        self.delete(TableId::<WaitableSet>::new(rep))?;

        Ok(())
    }

    pub(crate) fn waitable_join(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        waitable_handle: u32,
        set_handle: u32,
    ) -> Result<()> {
        let waitable = Waitable::from_instance(self, caller_instance, waitable_handle)?;

        let set = if set_handle == 0 {
            None
        } else {
            let (set, WaitableState::Set) =
                self.waitable_tables()[caller_instance].get_mut_by_index(set_handle)?
            else {
                bail!("invalid waitable-set handle");
            };

            Some(TableId::<WaitableSet>::new(set))
        };

        log::trace!(
            "waitable {} (handle {waitable_handle}) join set {:?} (handle {set_handle})",
            waitable.rep(),
            set.map(|v| v.rep())
        );

        waitable.join(self, set)
    }

    pub(crate) fn subtask_drop(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        task_id: u32,
    ) -> Result<()> {
        self.waitable_join(caller_instance, task_id, 0)?;

        let (rep, state) = self.waitable_tables()[caller_instance].remove_by_index(task_id)?;
        log::trace!("subtask_drop {rep} (handle {task_id})");
        let expected_caller_instance = match state {
            WaitableState::HostTask => self.get(TableId::<HostTask>::new(rep))?.caller_instance,
            WaitableState::GuestTask => {
                if let Caller::Guest { instance, .. } =
                    &self.get(TableId::<GuestTask>::new(rep))?.caller
                {
                    *instance
                } else {
                    unreachable!()
                }
            }
            _ => bail!("invalid task handle: {task_id}"),
        };
        assert_eq!(expected_caller_instance, caller_instance);
        Ok(())
    }

    pub(crate) fn context_get(&mut self, slot: u32) -> Result<u32> {
        let task = self.guest_task().unwrap();
        Ok(self.get(task)?.context[usize::try_from(slot).unwrap()])
    }

    pub(crate) fn context_set(&mut self, slot: u32, val: u32) -> Result<()> {
        let task = self.guest_task().unwrap();
        self.get_mut(task)?.context[usize::try_from(slot).unwrap()] = val;
        Ok(())
    }
}

impl Instance {
    /// Poll the specified future until it yields a result _or_ there are no more
    /// tasks to run in the `Store`.
    pub async fn get<U: Send, V: Send + Sync + 'static>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fut: impl Future<Output = V> + Send,
    ) -> Result<V> {
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[self.0].as_ref().unwrap().instance_ptr() };
        instance.poll_until(store, Box::pin(fut)).await
    }

    /// Wrap the specified future in a `Promise`.
    pub fn promise<U: Send, V: Send + Sync + 'static>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fut: impl Future<Output = V> + Send + 'static,
    ) -> Promise<V> {
        let store = store.as_context_mut();
        let instance = SendSyncPtr::new(
            NonNull::new(store.0[self.0].as_ref().unwrap().instance_ptr()).unwrap(),
        );
        let id = store.0.id();
        Promise {
            inner: Box::pin(fut),
            instance,
            id,
        }
    }

    #[doc(hidden)]
    pub fn spawn(
        &self,
        mut store: impl AsContextMut,
        task: impl std::future::Future<Output = Result<()>> + Send + 'static,
    ) {
        let instance = unsafe {
            &mut *store.as_context_mut().0[self.0]
                .as_ref()
                .unwrap()
                .instance_ptr()
        };
        instance.spawn(task)
    }
}

/// Trait representing component model ABI async intrinsics and fused adapter
/// helper functions.
pub unsafe trait VMComponentAsyncStore {
    /// A helper function for fused adapter modules involving calls where the
    /// caller is sync-lowered but the callee is async-lifted.
    fn sync_enter(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        string_encoding: u8,
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
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        string_encoding: u8,
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

    /// The `future.write` intrinsic.
    fn future_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        async_: bool,
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
        async_: bool,
        ty: TypeFutureTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32>;

    /// The `stream.write` intrinsic.
    fn stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        async_: bool,
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
        async_: bool,
        ty: TypeStreamTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32>;

    /// The "fast-path" implementation of the `stream.write` intrinsic for
    /// "flat" (i.e. memcpy-able) payloads.
    fn flat_stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        async_: bool,
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
        async_: bool,
        ty: TypeStreamTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
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
}

unsafe impl<T> VMComponentAsyncStore for StoreInner<T> {
    fn sync_enter(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        string_encoding: u8,
        result_count: u32,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()> {
        instance.enter_call::<T>(
            start,
            return_,
            caller_instance,
            task_return_type,
            memory,
            string_encoding,
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
        instance
            .exit_call(
                StoreContextMut(self),
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
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        string_encoding: u8,
        params: u32,
        results: u32,
    ) -> Result<()> {
        instance.enter_call::<T>(
            start,
            return_,
            caller_instance,
            task_return_type,
            memory,
            string_encoding,
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
        instance.exit_call(
            StoreContextMut(self),
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

    fn future_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        async_: bool,
        ty: TypeFutureTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32> {
        instance.guest_write(
            StoreContextMut(self),
            memory,
            realloc,
            string_encoding,
            async_,
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
        async_: bool,
        ty: TypeFutureTableIndex,
        future: u32,
        address: u32,
    ) -> Result<u32> {
        instance.guest_read(
            StoreContextMut(self),
            memory,
            realloc,
            string_encoding,
            async_,
            TableIndex::Future(ty),
            None,
            future,
            address,
            1,
        )
    }

    fn stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        async_: bool,
        ty: TypeStreamTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        instance.guest_write(
            StoreContextMut(self),
            memory,
            realloc,
            string_encoding,
            async_,
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
        async_: bool,
        ty: TypeStreamTableIndex,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        instance.guest_read(
            StoreContextMut(self),
            memory,
            realloc,
            string_encoding,
            async_,
            TableIndex::Stream(ty),
            None,
            stream,
            address,
            count,
        )
    }

    fn flat_stream_write(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        async_: bool,
        ty: TypeStreamTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        instance.guest_write(
            StoreContextMut(self),
            memory,
            realloc,
            StringEncoding::Utf8 as u8,
            async_,
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
        async_: bool,
        ty: TypeStreamTableIndex,
        payload_size: u32,
        payload_align: u32,
        stream: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        instance.guest_read(
            StoreContextMut(self),
            memory,
            realloc,
            StringEncoding::Utf8 as u8,
            async_,
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
        instance.error_context_debug_message(
            StoreContextMut(self),
            memory,
            realloc,
            string_encoding,
            ty,
            err_ctx_handle,
            debug_msg_address,
        )
    }
}

enum HostTaskOutput {
    Background(Result<()>),
    Waitable {
        waitable: Arc<AtomicU32>,
        fun: Box<dyn FnOnce(&mut ComponentInstance) -> Result<Event> + Send>,
    },
}

type HostTaskFuture = Pin<Box<dyn Future<Output = HostTaskOutput> + Send + 'static>>;

struct HostTask {
    common: WaitableCommon,
    caller_instance: RuntimeComponentInstanceIndex,
    has_suspended: bool,
}

impl HostTask {
    fn new(caller_instance: RuntimeComponentInstanceIndex) -> Self {
        Self {
            common: WaitableCommon::default(),
            caller_instance,
            has_suspended: false,
        }
    }
}

enum Deferred {
    None,
    Stackful {
        fiber: StoreFiber<'static>,
        async_: bool,
    },
    Stackless {
        call: Box<dyn FnOnce(&mut ComponentInstance) -> Result<u32> + Send + Sync + 'static>,
        instance: RuntimeComponentInstanceIndex,
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
    caller: fn(
        &mut ComponentInstance,
        RuntimeComponentInstanceIndex,
        SendSyncPtr<VMFuncRef>,
        Event,
        u32,
    ) -> Result<u32>,
    instance: RuntimeComponentInstanceIndex,
}

enum Caller {
    Host(Option<oneshot::Sender<LiftedResult>>),
    Guest {
        task: TableId<GuestTask>,
        instance: RuntimeComponentInstanceIndex,
    },
}

struct LiftResult {
    lift: RawLift,
    ty: TypeTupleIndex,
    memory: Option<SendSyncPtr<VMMemoryDefinition>>,
    string_encoding: StringEncoding,
}

struct GuestTask {
    common: WaitableCommon,
    lower_params: Option<RawLower>,
    lift_result: Option<LiftResult>,
    result: Option<LiftedResult>,
    callback: Option<Callback>,
    caller: Caller,
    deferred: Deferred,
    should_yield: bool,
    call_context: Option<CallContext>,
    sync_result: Option<Option<ValRaw>>,
    has_suspended: bool,
    context: [u32; 2],
    subtasks: HashSet<TableId<GuestTask>>,
    sync_call_set: TableId<WaitableSet>,
}

impl GuestTask {
    fn new(
        instance: &mut ComponentInstance,
        lower_params: RawLower,
        lift_result: LiftResult,
        caller: Caller,
        callback: Option<Callback>,
    ) -> Result<Self> {
        let sync_call_set = instance.push(WaitableSet::default())?;

        Ok(Self {
            common: WaitableCommon::default(),
            lower_params: Some(lower_params),
            lift_result: Some(lift_result),
            result: None,
            callback,
            caller,
            deferred: Deferred::None,
            should_yield: false,
            call_context: Some(CallContext::default()),
            sync_result: None,
            has_suspended: false,
            context: [0u32; 2],
            subtasks: HashSet::new(),
            sync_call_set,
        })
    }

    fn dispose(self, instance: &mut ComponentInstance, me: TableId<GuestTask>) -> Result<()> {
        instance.yielding().remove(&me);

        for waitable in mem::take(&mut instance.get_mut(self.sync_call_set)?.ready) {
            if let Some(Event::CallReturned) = waitable.common(instance)?.event {
                waitable.delete_from(instance)?;
            }
        }

        instance.delete(self.sync_call_set)?;

        if let Caller::Guest {
            task,
            instance: runtime_instance,
        } = &self.caller
        {
            let task_mut = instance.get_mut(*task)?;
            let present = task_mut.subtasks.remove(&me);
            assert!(present);

            for subtask in &self.subtasks {
                task_mut.subtasks.insert(*subtask);
            }

            for subtask in &self.subtasks {
                instance.get_mut(*subtask)?.caller = Caller::Guest {
                    task: *task,
                    instance: *runtime_instance,
                };
            }
        } else {
            for subtask in &self.subtasks {
                instance.get_mut(*subtask)?.caller = Caller::Host(None);
            }
        }

        Ok(())
    }
}

#[derive(Default)]
struct WaitableCommon {
    event: Option<Event>,
    set: Option<TableId<WaitableSet>>,
    rep: Arc<AtomicU32>,
}

impl Drop for WaitableCommon {
    fn drop(&mut self) {
        self.rep.store(0, Relaxed);
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
enum Waitable {
    Host(TableId<HostTask>),
    Guest(TableId<GuestTask>),
    Transmit(TableId<TransmitHandle>),
}

impl Waitable {
    fn from_instance(
        instance: &mut ComponentInstance,
        caller_instance: RuntimeComponentInstanceIndex,
        waitable: u32,
    ) -> Result<Self> {
        let (waitable, state) =
            instance.waitable_tables()[caller_instance].get_mut_by_index(waitable)?;

        Ok(match state {
            WaitableState::HostTask => Waitable::Host(TableId::new(waitable)),
            WaitableState::GuestTask => Waitable::Guest(TableId::new(waitable)),
            WaitableState::Stream(..) | WaitableState::Future(..) => {
                Waitable::Transmit(TableId::new(waitable))
            }
            _ => bail!("invalid waitable handle"),
        })
    }

    fn rep(&self) -> u32 {
        match self {
            Self::Host(id) => id.rep(),
            Self::Guest(id) => id.rep(),
            Self::Transmit(id) => id.rep(),
        }
    }

    fn join(
        &self,
        instance: &mut ComponentInstance,
        set: Option<TableId<WaitableSet>>,
    ) -> Result<()> {
        let old = mem::replace(&mut self.common(instance)?.set, set);

        if let Some(old) = old {
            match *self {
                Waitable::Host(id) => instance.remove_child(id, old),
                Waitable::Guest(id) => instance.remove_child(id, old),
                Waitable::Transmit(id) => instance.remove_child(id, old),
            }?;

            instance.get_mut(old)?.ready.remove(self);
        }

        if let Some(set) = set {
            match *self {
                Waitable::Host(id) => instance.add_child(id, set),
                Waitable::Guest(id) => instance.add_child(id, set),
                Waitable::Transmit(id) => instance.add_child(id, set),
            }?;

            if self.common(instance)?.event.is_some() {
                self.mark_ready(instance)?;
            }
        }

        Ok(())
    }

    fn has_suspended(&self, instance: &mut ComponentInstance) -> Result<bool> {
        Ok(match self {
            Waitable::Host(id) => instance.get(*id)?.has_suspended,
            Waitable::Guest(id) => instance.get(*id)?.has_suspended,
            Waitable::Transmit(_) => true,
        })
    }

    fn common<'a>(&self, instance: &'a mut ComponentInstance) -> Result<&'a mut WaitableCommon> {
        Ok(match self {
            Self::Host(id) => &mut instance.get_mut(*id)?.common,
            Self::Guest(id) => &mut instance.get_mut(*id)?.common,
            Self::Transmit(id) => &mut instance.get_mut(*id)?.common,
        })
    }

    fn mark_ready(&self, instance: &mut ComponentInstance) -> Result<()> {
        if let Some(set) = self.common(instance)?.set {
            instance.get_mut(set)?.ready.insert(*self);
        }
        Ok(())
    }

    fn send_or_mark_ready(&self, instance: &mut ComponentInstance) -> Result<()> {
        if let Some(set) = self.common(instance)?.set {
            if let Some(task) = instance.get_mut(set)?.waiting.pop_first() {
                let event = self.common(instance)?.event.take().unwrap();
                instance.maybe_send_event(task, event, Some(*self))?;
            } else {
                instance.get_mut(set)?.ready.insert(*self);
            }
        }
        Ok(())
    }

    fn on_delivery(&self, instance: &mut ComponentInstance, event: Event) {
        match event {
            Event::FutureRead { ty, handle, .. }
            | Event::FutureWrite {
                pending: Some((ty, handle)),
                ..
            } => {
                let runtime_instance = instance.component_types()[ty].instance;
                let (rep, WaitableState::Future(actual_ty, state)) = instance.waitable_tables()
                    [runtime_instance]
                    .get_mut_by_index(handle)
                    .unwrap()
                else {
                    unreachable!()
                };
                assert_eq!(*actual_ty, ty);
                assert_eq!(rep, self.rep());
                assert_eq!(*state, StreamFutureState::Busy);
                *state = match event {
                    Event::FutureRead { .. } => StreamFutureState::Read,
                    Event::FutureWrite { .. } => StreamFutureState::Write,
                    _ => unreachable!(),
                };
            }
            Event::StreamRead { ty, handle, .. }
            | Event::StreamWrite {
                pending: Some((ty, handle)),
                ..
            } => {
                let runtime_instance = instance.component_types()[ty].instance;
                let (rep, WaitableState::Stream(actual_ty, state)) = instance.waitable_tables()
                    [runtime_instance]
                    .get_mut_by_index(handle)
                    .unwrap()
                else {
                    unreachable!()
                };
                assert_eq!(*actual_ty, ty);
                assert_eq!(rep, self.rep());
                assert_eq!(*state, StreamFutureState::Busy);
                *state = match event {
                    Event::StreamRead { .. } => StreamFutureState::Read,
                    Event::StreamWrite { .. } => StreamFutureState::Write,
                    _ => unreachable!(),
                };
            }
            _ => {}
        }
    }

    fn delete_from(&self, instance: &mut ComponentInstance) -> Result<()> {
        match self {
            Self::Host(task) => {
                log::trace!("delete host task {}", task.rep());
                instance.delete(*task)?;
            }
            Self::Guest(task) => {
                log::trace!("delete guest task {}", task.rep());
                instance.delete(*task)?.dispose(instance, *task)?;
            }
            Self::Transmit(task) => {
                instance.delete(*task)?;
            }
        }

        Ok(())
    }
}

#[derive(Default)]
struct WaitableSet {
    ready: BTreeSet<Waitable>,
    waiting: BTreeSet<TableId<GuestTask>>,
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
    Box<dyn FnOnce(&mut ComponentInstance, &mut [MaybeUninit<ValRaw>]) -> Result<()> + Send + Sync>;

type LowerFn = fn(LiftLowerContext, *mut dyn VMStore, &mut [MaybeUninit<ValRaw>]) -> Result<()>;

type RawLift = Box<
    dyn FnOnce(&mut ComponentInstance, &[ValRaw]) -> Result<Box<dyn Any + Send + Sync>>
        + Send
        + Sync,
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

pub(crate) struct AsyncState {
    current_suspend: UnsafeCell<
        *mut Suspend<
            (Option<*mut dyn VMStore>, Result<()>),
            Option<*mut dyn VMStore>,
            (Option<*mut dyn VMStore>, Result<()>),
        >,
    >,
    current_poll_cx: UnsafeCell<PollContext>,
}

impl Default for AsyncState {
    fn default() -> Self {
        Self {
            current_suspend: UnsafeCell::new(ptr::null_mut()),
            current_poll_cx: UnsafeCell::new(PollContext::default()),
        }
    }
}

impl AsyncState {
    pub(crate) fn async_guard_range(&self) -> Range<*mut u8> {
        let context = unsafe { *self.current_poll_cx.get() };
        context.guard_range_start..context.guard_range_end
    }
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
    pub(crate) fn new(store: &mut StoreOpaque) -> Self {
        Self::try_new(store).unwrap()
    }

    pub(crate) fn try_new(store: &mut StoreOpaque) -> Option<Self> {
        let current_poll_cx = store.concurrent_async_state().current_poll_cx.get();
        if unsafe { (*current_poll_cx).future_context.is_null() } {
            None
        } else {
            Some(Self {
                current_suspend: store.concurrent_async_state().current_suspend.get(),
                current_stack_limit: store.vm_store_context().stack_limit.get(),
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

    pub(crate) unsafe fn block_on<U>(
        &self,
        mut future: Pin<&mut (dyn Future<Output = U> + Send)>,
        mut store: Option<*mut dyn VMStore>,
    ) -> Result<(U, Option<*mut dyn VMStore>)> {
        loop {
            match self.poll(future.as_mut()) {
                Poll::Ready(v) => break Ok((v, store)),
                Poll::Pending => {}
            }

            store = self.suspend(store)?;
        }
    }

    unsafe fn suspend(&self, store: Option<*mut dyn VMStore>) -> Result<Option<*mut dyn VMStore>> {
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

pub struct ConcurrentState {
    guest_task: Option<TableId<GuestTask>>,
    futures: Mutex<ReadyChunks<FuturesUnordered<HostTaskFuture>>>,
    table: Table,
    // TODO: this can and should be a `PrimaryMap`
    instance_states: HashMap<RuntimeComponentInstanceIndex, InstanceState>,
    yielding: HashMap<TableId<GuestTask>, Option<TableId<WaitableSet>>>,
    unblocked: HashSet<RuntimeComponentInstanceIndex>,
    waitable_tables: PrimaryMap<RuntimeComponentInstanceIndex, StateTable<WaitableState>>,

    /// (Sub)Component specific error context tracking
    ///
    /// At the component level, only the number of references (`usize`) to a given error context is tracked,
    /// with state related to the error context being held at the component model level, in concurrent
    /// state.
    ///
    /// The state tables in the (sub)component local tracking must contain a pointer into the global
    /// error context lookups in order to ensure that in contexts where only the local reference is present
    /// the global state can still be maintained/updated.
    error_context_tables:
        PrimaryMap<TypeComponentLocalErrorContextTableIndex, StateTable<LocalErrorContextRefCount>>,

    /// Reference counts for all component error contexts
    ///
    /// NOTE: it is possible the global ref count to be *greater* than the sum of
    /// (sub)component ref counts as tracked by `error_context_tables`, for
    /// example when the host holds one or more references to error contexts.
    ///
    /// The key of this primary map is often referred to as the "rep" (i.e. host-side
    /// component-wide representation) of the index into concurrent state for a given
    /// stored `ErrorContext`.
    ///
    /// Stated another way, `TypeComponentGlobalErrorContextTableIndex` is essentially the same
    /// as a `TableId<ErrorContextState>`.
    global_error_context_ref_counts:
        BTreeMap<TypeComponentGlobalErrorContextTableIndex, GlobalErrorContextRefCount>,
}

impl ConcurrentState {
    pub(crate) fn new(num_waitable_tables: u32, num_error_context_tables: usize) -> Self {
        let mut waitable_tables =
            PrimaryMap::with_capacity(usize::try_from(num_waitable_tables).unwrap());
        for _ in 0..num_waitable_tables {
            waitable_tables.push(StateTable::default());
        }

        let mut error_context_tables = PrimaryMap::<
            TypeComponentLocalErrorContextTableIndex,
            StateTable<LocalErrorContextRefCount>,
        >::with_capacity(num_error_context_tables);
        for _ in 0..num_error_context_tables {
            error_context_tables.push(StateTable::default());
        }

        Self {
            guest_task: None,
            table: Table::new(),
            futures: Mutex::new(ReadyChunks::new(FuturesUnordered::new(), 1024)),
            instance_states: HashMap::new(),
            yielding: HashMap::new(),
            unblocked: HashSet::new(),
            waitable_tables,
            error_context_tables,
            global_error_context_ref_counts: BTreeMap::new(),
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
    F: FnOnce(&mut ComponentInstance, &mut [MaybeUninit<ValRaw>]) -> Result<()> + Send + Sync,
>(
    fun: F,
) -> F {
    fun
}

fn for_any_lift<
    F: FnOnce(&mut ComponentInstance, &[ValRaw]) -> Result<Box<dyn Any + Send + Sync>> + Send + Sync,
>(
    fun: F,
) -> F {
    fun
}

pub(crate) async fn on_fiber<R: Send + 'static, T: Send>(
    store: StoreContextMut<'_, T>,
    instance: Option<RuntimeComponentInstanceIndex>,
    func: impl FnOnce(&mut StoreContextMut<T>) -> R + Send,
) -> Result<R> {
    unsafe {
        on_fiber_raw(VMStoreRawPtr(store.traitobj()), instance, move |store| {
            func(&mut StoreContextMut(&mut *store.cast()))
        })
        .await
    }
}

async unsafe fn on_fiber_raw<R: Send + 'static>(
    store: VMStoreRawPtr,
    instance: Option<RuntimeComponentInstanceIndex>,
    func: impl FnOnce(*mut dyn VMStore) -> R + Send,
) -> Result<R> {
    let result = Arc::new(Mutex::new(None));
    let mut fiber = make_fiber(store.0.as_ptr(), instance, {
        let result = result.clone();
        move |store| {
            *result.lock().unwrap() = Some(func(store));
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

    poll_fn(store, guard_range, move |_, mut store| {
        match resume_fiber(&mut fiber, store.take(), Ok(())) {
            Ok(Ok((store, result))) => Ok(result.map(|()| store)),
            Ok(Err(s)) => Err(s),
            Err(e) => Ok(Err(e)),
        }
    })
    .await?;

    let result = result.lock().unwrap().take().unwrap();
    Ok(result)
}

fn unpack_callback_code(code: u32) -> (u32, u32) {
    (code & 0xF, code >> 4)
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

impl Drop for StoreFiber<'_> {
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

unsafe impl Send for StoreFiber<'_> {}
unsafe impl Sync for StoreFiber<'_> {}

unsafe fn make_fiber<'a>(
    store: *mut dyn VMStore,
    instance: Option<RuntimeComponentInstanceIndex>,
    fun: impl FnOnce(*mut dyn VMStore) -> Result<()> + 'a,
) -> Result<StoreFiber<'a>> {
    let engine = (*store).engine().clone();
    let stack = engine.allocator().allocate_fiber_stack()?;
    Ok(StoreFiber {
        fiber: Some(Fiber::new(
            stack,
            move |(store, result): (Option<*mut dyn VMStore>, Result<()>), suspend| {
                if result.is_err() {
                    (store, result)
                } else {
                    let store = store.unwrap();
                    let suspend_ptr = (*store)
                        .store_opaque_mut()
                        .concurrent_async_state()
                        .current_suspend
                        .get();
                    let _reset = Reset(suspend_ptr, *suspend_ptr);
                    *suspend_ptr = suspend;
                    (Some(store), fun(store))
                }
            },
        )?),
        state: Some(AsyncWasmCallState::new()),
        engine,
        suspend: (*store)
            .store_opaque_mut()
            .concurrent_async_state()
            .current_suspend
            .get(),
        stack_limit: (*store).vm_store_context().stack_limit.get(),
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

fn resume_fiber(
    fiber: &mut StoreFiber,
    store: Option<*mut dyn VMStore>,
    result: Result<()>,
) -> Result<Result<(*mut dyn VMStore, Result<()>), Option<*mut dyn VMStore>>> {
    unsafe {
        match resume_fiber_raw(fiber, store, result).map(|(store, result)| (store.unwrap(), result))
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

unsafe fn suspend_fiber(
    suspend: *mut *mut Suspend<
        (Option<*mut dyn VMStore>, Result<()>),
        Option<*mut dyn VMStore>,
        (Option<*mut dyn VMStore>, Result<()>),
    >,
    stack_limit: *mut usize,
    store: Option<*mut dyn VMStore>,
) -> Result<Option<*mut dyn VMStore>> {
    let _reset_suspend = Reset(suspend, *suspend);
    let _reset_stack_limit = Reset(stack_limit, *stack_limit);
    assert!(!(*suspend).is_null());
    let (store, result) = (**suspend).suspend(store);
    result?;
    Ok(store)
}

pub(crate) struct WaitableCheckParams {
    set: TableId<WaitableSet>,
    memory: *mut VMMemoryDefinition,
    payload: u32,
    caller_instance: RuntimeComponentInstanceIndex,
}

pub(crate) enum WaitableCheck {
    Wait(WaitableCheckParams),
    Poll(WaitableCheckParams),
    Yield,
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
    &mut ComponentInstance,
) -> Result<[MaybeUninit<ValRaw>; MAX_FLAT_PARAMS]>
       + Send
       + Sync
       + 'static {
    move |mut store: StoreContextMut<T>, instance: &mut ComponentInstance| {
        if !instance.may_enter(guest_task, callee_instance) {
            bail!(crate::Trap::CannotEnterComponent);
        }

        let mut storage = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
        let lower = instance.get_mut(guest_task)?.lower_params.take().unwrap();
        lower(instance, &mut storage[..param_count])?;

        unsafe {
            if let Some(mut flags) = flags {
                flags.set_may_enter(false);
            }
            crate::Func::call_unchecked_raw(
                &mut store,
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

        Ok(storage)
    }
}

pub(crate) fn start_call<T: Send, LowerParams: Copy, R: 'static>(
    mut store: StoreContextMut<T>,
    lower_params: LowerFn,
    lower_context: LiftLowerContext,
    lift_result: LiftFn,
    lift_context: LiftLowerContext,
    handle: Func,
) -> Result<Promise<R>> {
    let func_data = &store.0[handle.0];
    let task_return_type = func_data.types[func_data.ty].results;
    let is_concurrent = func_data.options.async_();
    let component_instance = func_data.component_instance;
    let instance = func_data.instance;
    let callee = func_data.export.func_ref;
    let callback = func_data.options.callback;
    let post_return = func_data.post_return;
    let memory = func_data.options.memory.map(SendSyncPtr::new);
    let string_encoding = func_data.options.string_encoding();

    let instance = unsafe { &mut *store.0[instance.0].as_ref().unwrap().instance_ptr() };

    assert!(instance.guest_task().is_none());

    let (tx, rx) = oneshot::channel();

    let task = GuestTask::new(
        instance,
        Box::new(for_any_lower(move |instance, params| {
            lower_params(lower_context, instance.store(), params)
        })),
        LiftResult {
            lift: Box::new(for_any_lift(move |instance, result| {
                lift_result(lift_context, instance.store(), result)
            })),
            ty: task_return_type,
            memory,
            string_encoding,
        },
        Caller::Host(Some(tx)),
        callback.map(|v| Callback {
            function: SendSyncPtr::new(v),
            caller: ComponentInstance::call_callback::<T>,
            instance: component_instance,
        }),
    )?;

    let guest_task = instance.push(task)?;
    instance
        .get_mut(guest_task)?
        .common
        .rep
        .store(guest_task.rep(), Relaxed);

    log::trace!("starting call {}", guest_task.rep());

    let call = make_call(
        guest_task,
        SendSyncPtr::new(callee),
        component_instance,
        mem::size_of::<LowerParams>() / mem::size_of::<ValRaw>(),
        1,
        if callback.is_none() {
            None
        } else {
            Some(instance.instance_flags(component_instance))
        },
    );

    *instance.guest_task() = Some(guest_task);
    log::trace!(
        "pushed {} as current task; old task was None",
        guest_task.rep()
    );

    instance.do_start_call(
        store.as_context_mut(),
        guest_task,
        is_concurrent,
        call,
        callback.map(SendSyncPtr::new),
        post_return.map(|f| SendSyncPtr::new(f.func_ref)),
        component_instance,
        1,
        false,
    )?;

    *instance.guest_task() = None;
    log::trace!("popped current task {}; new task is None", guest_task.rep());

    log::trace!("started call {}", guest_task.rep());

    Ok(Promise {
        inner: Box::pin(rx.map(|result| *result.unwrap().downcast().unwrap())),
        instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
        id: store.0.id(),
    })
}

pub(crate) fn call<T: Send, LowerParams: Copy, R: Send + Sync + 'static>(
    mut store: StoreContextMut<T>,
    lower_params: LowerFn,
    lower_context: LiftLowerContext,
    lift_result: LiftFn,
    lift_context: LiftLowerContext,
    handle: Func,
) -> Result<R> {
    let promise = start_call::<_, LowerParams, R>(
        store.as_context_mut(),
        lower_params,
        lower_context,
        lift_result,
        lift_context,
        handle,
    )?;

    let instance = unsafe {
        &mut *store.0[store.0[handle.0].instance.0]
            .as_ref()
            .unwrap()
            .instance_ptr()
    };
    instance.loop_until(promise.into_future())
}

async unsafe fn poll_fn<R>(
    store: VMStoreRawPtr,
    guard_range: (Option<SendSyncPtr<u8>>, Option<SendSyncPtr<u8>>),
    mut fun: impl FnMut(&mut Context, Option<*mut dyn VMStore>) -> Result<R, Option<*mut dyn VMStore>>,
) -> R {
    #[derive(Clone, Copy)]
    struct PollCx(*mut PollContext);

    unsafe impl Send for PollCx {}

    let poll_cx = PollCx(
        (*store.0.as_ptr())
            .store_opaque_mut()
            .concurrent_async_state()
            .current_poll_cx
            .get(),
    );
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

            match fun(cx, store.take().map(|s| s.0.as_ptr())) {
                Ok(v) => Poll::Ready(v),
                Err(s) => {
                    store = s.map(|s| VMStoreRawPtr(NonNull::new(s).unwrap()));
                    Poll::Pending
                }
            }
        }
    })
    .await
}
