use {
    crate::{
        component::{
            func::{self, Func, Options},
            Instance, Lift, Lower, Val,
        },
        store::{StoreInner, StoreOpaque},
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
    futures_and_streams::{FlatAbi, ReturnCode, StreamFutureState, TableIndex, TransmitHandle},
    once_cell::sync::Lazy,
    ready_chunks::ReadyChunks,
    states::StateTable,
    std::{
        any::Any,
        borrow::ToOwned,
        boxed::Box,
        cell::{Cell, RefCell, UnsafeCell},
        collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
    ErrorContext, FutureReader, FutureWriter, HostFuture, HostStream, ReadBuffer, StreamReader,
    StreamWriter, VecBuffer, Watch, WriteBuffer,
};

mod error_contexts;
mod futures_and_streams;
mod ready_chunks;
mod states;
mod table;

const BLOCKED: u32 = 0xffff_ffff;

/// Corresponds to `CallState` in the upstream spec.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Status {
    Starting = 0,
    Started = 1,
    Returned = 2,
    StartCancelled = 3,
    ReturnCancelled = 4,
}

impl Status {
    /// Packs this status and the optional `waitable` provided into a 32-bit
    /// result that the canonical ABI requires.
    ///
    /// The low 4 bits are reserved for the status while the upper 28 bits are
    /// the waitable, if present.
    pub fn pack(self, waitable: Option<u32>) -> u32 {
        assert!(matches!(self, Status::Returned) == waitable.is_none());
        let waitable = waitable.unwrap_or(0);
        assert!(waitable < (1 << 28));
        (waitable << 4) | (self as u32)
    }
}

#[derive(Clone, Copy, Debug)]
enum Event {
    None,
    Cancelled,
    Subtask {
        status: Status,
    },
    StreamRead {
        code: ReturnCode,
        handle: u32,
        ty: TypeStreamTableIndex,
    },
    StreamWrite {
        code: ReturnCode,
        pending: Option<(TypeStreamTableIndex, u32)>,
    },
    FutureRead {
        code: ReturnCode,
        handle: u32,
        ty: TypeFutureTableIndex,
    },
    FutureWrite {
        code: ReturnCode,
        pending: Option<(TypeFutureTableIndex, u32)>,
    },
}

impl Event {
    fn parts(self) -> (u32, u32) {
        const EVENT_NONE: u32 = 0;
        const EVENT_SUBTASK: u32 = 1;
        const EVENT_STREAM_READ: u32 = 2;
        const EVENT_STREAM_WRITE: u32 = 3;
        const EVENT_FUTURE_READ: u32 = 4;
        const EVENT_FUTURE_WRITE: u32 = 5;
        const EVENT_CANCELLED: u32 = 6;
        match self {
            Event::None => (EVENT_NONE, 0),
            Event::Cancelled => (EVENT_CANCELLED, 0),
            Event::Subtask { status } => (EVENT_SUBTASK, status as u32),
            Event::StreamRead { code, .. } => (EVENT_STREAM_READ, code.encode()),
            Event::StreamWrite { code, .. } => (EVENT_STREAM_WRITE, code.encode()),
            Event::FutureRead { code, .. } => (EVENT_FUTURE_READ, code.encode()),
            Event::FutureWrite { code, .. } => (EVENT_FUTURE_WRITE, code.encode()),
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
    pub fn spawn(&mut self, task: impl AccessorTask<T, U, Result<()>>) -> AbortHandle
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
    ///
    /// The returned [`AbortHandle`] may be used to cancel the task.
    pub fn spawn(&mut self, task: impl AccessorTask<T, U, Result<()>>) -> AbortHandle {
        let mut accessor = Self {
            get: self.get.clone(),
            spawn: self.spawn,
            instance: self.instance,
            _phantom: PhantomData,
        };
        let future = Arc::new(Mutex::new(AbortWrapper::Unpolled(unsafe {
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
        let handle = AbortHandle::new(future.clone());
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

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[doc(hidden)]
pub enum AbortWrapper<T> {
    Unpolled(BoxFuture<T>),
    Polled { future: BoxFuture<T>, waker: Waker },
    Aborted,
}

impl<T> AbortWrapper<T> {
    fn abort(&mut self) {
        match mem::replace(self, Self::Aborted) {
            Self::Unpolled(_) | Self::Aborted => {}
            Self::Polled { waker, .. } => waker.wake(),
        }
    }
}

type AbortWrapped<T> = Arc<Mutex<AbortWrapper<T>>>;

#[doc(hidden)]
pub type Spawned = AbortWrapped<Result<()>>;

type AbortFunction = Arc<dyn Fn() + Send + Sync + 'static>;

/// Handle to a task which may be used to abort it.
pub struct AbortHandle(AbortFunction);

impl AbortHandle {
    fn new<T: 'static>(wrapped: AbortWrapped<T>) -> Self {
        Self(Arc::new(move || wrapped.try_lock().unwrap().abort()))
    }

    /// Abort the task.
    ///
    /// This will panic if called from within the spawned task itself.
    pub fn abort(&self) {
        (self.0)();
    }

    /// Return an [`AbortOnDropHandle`] which, when dropped, will abort the
    /// task.
    ///
    /// The returned instance will panic if dropped from within the spawned task
    /// itself.
    pub fn abort_on_drop_handle(&self) -> AbortOnDropHandle {
        AbortOnDropHandle(self.0.clone())
    }
}

/// Handle to a spawned task which will abort the task when dropped.
pub struct AbortOnDropHandle(AbortFunction);

impl Drop for AbortOnDropHandle {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// Represents a task which may be provided to `Accessor::spawn`,
/// `Accessor::forward`, or `Instance::spawn`.
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
    static STATE: RefCell<Option<State>> = RefCell::new(None);
    static INSTANCE: Cell<*mut ComponentInstance> = Cell::new(ptr::null_mut());
}

struct ResetState(Option<State>);

impl Drop for ResetState {
    fn drop(&mut self) {
        STATE.with(|v| {
            *v.borrow_mut() = self.0.take();
        })
    }
}

struct ResetInstance(Option<SendSyncPtr<ComponentInstance>>);

impl Drop for ResetInstance {
    fn drop(&mut self) {
        INSTANCE.with(|v| v.set(self.0.map(|v| v.as_ptr()).unwrap_or_else(ptr::null_mut)))
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
        let old_state = STATE.with(|v| {
            v.replace(Some(State {
                host: (host as *mut T).cast(),
                store: store.0.as_ptr().cast(),
                spawned: Vec::new(),
            }))
        });
        let _reset_state = ResetState(old_state);
        let old_instance = INSTANCE.with(|v| v.replace(instance.as_ptr()));
        let _reset_instance = ResetInstance(NonNull::new(old_instance).map(SendSyncPtr::new));
        (future.poll(cx), STATE.with(|v| v.take()).unwrap().spawned)
    };

    let instance_ref = unsafe { &mut *instance.as_ptr() };
    for spawned in spawned {
        instance_ref.spawn(future::poll_fn(move |cx| {
            let mut spawned = spawned.try_lock().unwrap();
            let inner = mem::replace(DerefMut::deref_mut(&mut spawned), AbortWrapper::Aborted);
            if let AbortWrapper::Unpolled(mut future) | AbortWrapper::Polled { mut future, .. } =
                inner
            {
                let result = poll_with_state::<T, _>(store, instance, cx, future.as_mut());
                *DerefMut::deref_mut(&mut spawned) = AbortWrapper::Polled {
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
    /// Represents a waitable-set handle.
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

    // This is called immediately after the fiber belonging to the specified
    // guest task suspends itself, in which case we may need to link that task
    // to the task which was running when it suspending.
    //
    // For example, if we resume a fiber for task 4, and it makes an
    // async-lowered call to an async-lifted task 5, which makes a sync-lowered
    // call to a concurrent host function, creating task 6, which suspends, then
    // we'll point task 5 at task 4 as a reminder that the `Status::Returned`
    // event produced by the completion of 6 should be sent to 5 by resuming the
    // fiber belonging to 4 (and _not_ by attempting to call task 5's callback,
    // which would fail due to a reentrance violation).
    fn maybe_defer_to(&mut self, guest_task: TableId<GuestTask>) -> Result<()> {
        if let Some(current) = self.guest_task() {
            let current = *current;
            if current != guest_task {
                let task = self.get_mut(current)?;
                if let Deferred::None = &task.deferred {
                    task.deferred = Deferred::Caller(guest_task);
                }
            }
        }

        Ok(())
    }

    fn may_enter(&mut self, mut guest_task: TableId<GuestTask>) -> bool {
        let guest_instance = self.get(guest_task).unwrap().instance;

        // Walk the task tree back to the root, looking for potential
        // reentrance.
        //
        // TODO: This could be optimized by maintaining a per-`GuestTask` bitset
        // such that each bit represents and instance which has been entered by
        // that task or an ancestor of that task, in which case this would be a
        // constant time check.
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

        // We can only send an event to a callback if the callee has suspended
        // at least once; otherwise, we will not have had a chance to return a
        // `waitable` handle to the caller yet, in which case we can only queue
        // the event for later.
        let may_send = waitable
            .map(|v| v.has_suspended(self))
            .unwrap_or(Ok(true))?;

        let task = self.get_mut(guest_task)?;
        if let (true, Some(callback)) = (may_send, task.callback) {
            let instance = task.instance;
            let handle = if let Some(waitable) = waitable {
                let Some((
                    handle,
                    WaitableState::HostTask
                    | WaitableState::GuestTask
                    | WaitableState::Stream(..)
                    | WaitableState::Future(..),
                )) = self.waitable_tables()[instance].get_mut_by_rep(waitable.rep())
                else {
                    // If there's no handle found for the subtask, that means either the
                    // subtask was closed by the caller already or the caller called the
                    // callee via a sync-lowered import.  Either way, we can skip this
                    // event.
                    if let Event::Subtask {
                        status: Status::Returned | Status::ReturnCancelled,
                    } = event
                    {
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

            let code = (callback.caller)(self, instance, callback.function, event, handle)?;

            self.maybe_pop_call_context(guest_task)?;

            self.handle_callback_code(instance, guest_task, code, Event::None)?;

            *self.guest_task() = old_task;
            log::trace!(
                "maybe_send_event (callback): restored {:?} as current task",
                old_task.map(|v| v.rep())
            );
        } else if let Some(waitable) = waitable {
            waitable.set_event(self, Some(event))?;
            waitable.mark_ready(self)?;

            let resumed = if let Event::Subtask {
                status: Status::Returned | Status::ReturnCancelled,
            } = event
            {
                if let Some((fiber, async_, owner)) = self.take_stackful(guest_task)? {
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
                    self.resume_stackful(owner, fiber, async_)?;
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
                // TODO: resume stackful task if it is blocked on e.g. `waitable_set_wait`
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
            log::trace!("queue event {event:?} to {}", guest_task.rep());

            task.event = Some(event);
            // TODO: resume stackful task if it is blocked on e.g. `waitable_set_wait`
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
                                waitable.set_event(
                                    self,
                                    Some(Event::Subtask {
                                        status: Status::Returned,
                                    }),
                                )?;
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
                        self.maybe_defer_to(guest_task)?;
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
            Event::None
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
            waitable.set_event(self, Some(event))?;
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

        self.handle_callback_code(
            runtime_instance,
            guest_task,
            code,
            Event::Subtask {
                status: Status::Started,
            },
        )?;

        self.maybe_resume_next_task(runtime_instance)
    }

    fn poll_for_result(&mut self, task: TableId<GuestTask>) -> Result<()> {
        self.poll_loop(move |instance| {
            Ok::<_, anyhow::Error>(instance.get(task)?.result.is_none())
        })?;

        if self.get(task)?.result.is_some() {
            Ok(())
        } else {
            Err(anyhow!(crate::Trap::NoAsyncResult))
        }
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
                            Event::Subtask {
                                status: Status::Returned,
                            } => Waitable::Host(TableId::<HostTask>::new(waitable)),
                            Event::StreamRead { .. }
                            | Event::FutureRead { .. }
                            | Event::StreamWrite { .. }
                            | Event::FutureWrite { .. } => {
                                Waitable::Transmit(TableId::<TransmitHandle>::new(waitable))
                            }
                            _ => unreachable!(),
                        };
                        waitable.set_event(self, Some(event))?;
                        waitable.send_or_mark_ready(self)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn take_stackful(
        &mut self,
        task: TableId<GuestTask>,
    ) -> Result<Option<(StoreFiber<'static>, bool, TableId<GuestTask>)>> {
        Ok(match &self.get(task)?.deferred {
            Deferred::Stackful { .. } => {
                let Deferred::Stackful { fiber, async_ } =
                    mem::replace(&mut self.get_mut(task)?.deferred, Deferred::None)
                else {
                    unreachable!()
                };
                Some((fiber, async_, task))
            }
            Deferred::Caller(caller) => {
                let caller = *caller;
                self.get_mut(task)?.deferred = Deferred::None;
                let Deferred::Stackful { fiber, async_ } =
                    mem::replace(&mut self.get_mut(caller)?.deferred, Deferred::None)
                else {
                    unreachable!()
                };
                Some((fiber, async_, caller))
            }
            _ => None,
        })
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

                if let Some((fiber, async_, owner)) = self.take_stackful(guest_task)? {
                    resumed = true;
                    let old_task = self.guest_task().replace(guest_task);
                    log::trace!(
                        "unyield: replaced {:?} with {} as current task",
                        old_task.map(|v| v.rep()),
                        guest_task.rep()
                    );
                    self.resume_stackful(owner, fiber, async_)?;
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
                if let Some(task) = entry.pending.pop_first() {
                    resumed = true;
                    self.resume(task)?;
                }
            }
        }

        for fun in mem::take(&mut self.concurrent_state.deferred) {
            resumed = true;
            fun(self)?;
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
            Deferred::Caller(caller) => {
                let Deferred::Stackful { fiber, async_ } =
                    mem::replace(&mut self.get_mut(caller)?.deferred, Deferred::None)
                else {
                    unreachable!()
                };
                self.resume_stackful(task, fiber, async_)
            }
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
            if let Some(next) = state.pending.pop_first() {
                self.resume(next)
            } else {
                Ok(())
            }
        }
    }

    pub(crate) fn loop_until<R>(
        &mut self,
        mut future: Pin<Box<dyn Future<Output = R> + Send + '_>>,
    ) -> Result<R> {
        let result = Arc::new(Mutex::new(None));
        let poll = &mut {
            let result = result.clone();
            move |instance: &mut ComponentInstance| {
                Ok(if result.try_lock().unwrap().is_none() {
                    let ready = unsafe {
                        let cx = AsyncCx::new((*instance.store()).store_opaque_mut());
                        cx.poll(future.as_mut())
                    };
                    match ready {
                        Poll::Ready(value) => {
                            *result.try_lock().unwrap() = Some(value);
                            false
                        }
                        Poll::Pending => true,
                    }
                } else {
                    false
                })
            }
        };

        // Poll once outside the event loop for possible side effects
        // (e.g. creating work for the event loop to do).
        //
        // If this yields a result, we will return it without running the event
        // loop at all.
        poll(self)?;

        if result.try_lock().unwrap().is_none() {
            self.poll_loop(|instance| poll(instance))?;

            // Poll once more to get the result if necessary:
            if result.try_lock().unwrap().is_none() {
                poll(self)?;
            }
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
        result_count: usize,
        use_fiber: bool,
    ) -> Result<()> {
        let callee_instance = self.get(guest_task)?.instance;
        let state = &mut self.instance_states().entry(callee_instance).or_default();
        let ready = state.pending.is_empty() && !(state.backpressure || state.in_sync_call);

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
                log::trace!(
                    "deferring stackless call for {}; pending: {}, backpressure: {} in_sync_call: {}",
                    guest_task.rep(),
                    state.pending.len(),
                    state.backpressure,
                    state.in_sync_call
                );

                self.instance_states()
                    .get_mut(&callee_instance)
                    .unwrap()
                    .pending
                    .insert(guest_task);

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

                    instance.task_complete(guest_task, result, Status::Returned)?;
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
                                    self.maybe_defer_to(guest_task)?;
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
                        .pending
                        .insert(guest_task);

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
                self.handle_callback_code(
                    callee_instance,
                    guest_task,
                    code,
                    Event::Subtask {
                        status: Status::Started,
                    },
                )?;
            }

            Ok(())
        }
    }

    fn enter_call<T>(
        &mut self,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        callee_instance: RuntimeComponentInstanceIndex,
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
                    waitable.set_event(
                        instance,
                        Some(Event::Subtask {
                            status: Status::Started,
                        }),
                    )?;
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
                    Ok(Box::new(DummyResult) as Box<dyn Any + Send + Sync>)
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
            callee_instance,
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
        callee: *mut VMFuncRef,
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

        let task = self.get_mut(guest_task)?;
        if !callback.is_null() {
            task.callback = Some(Callback {
                function: SendSyncPtr::new(NonNull::new(callback).unwrap()),
                caller: Self::call_callback::<T>,
            });
        }

        let callee_instance = task.instance;

        let call = make_call(
            guest_task,
            callee,
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
            result_count,
            true,
        )?;

        let task = self.get(guest_task)?;

        let mut status = if task.lower_params.is_some() {
            Status::Starting
        } else if task.lift_result.is_some() {
            let event = Waitable::Guest(guest_task).take_event(self)?;
            assert!(matches!(
                event,
                Some(Event::Subtask {
                    status: Status::Started,
                })
            ));
            Status::Started
        } else {
            let event = Waitable::Guest(guest_task).take_event(self)?;
            assert!(matches!(
                event,
                Some(Event::Subtask {
                    status: Status::Returned,
                })
            ));
            Status::Returned
        };

        log::trace!("status {status:?} for {}", guest_task.rep());

        let waitable = if status != Status::Returned {
            if async_caller {
                let task = self.get_mut(guest_task)?;
                task.has_suspended = true;
                let Caller::Guest { instance, .. } = &task.caller else {
                    // As of this writing, `exit_call` is only used for
                    // guest->guest calls.
                    unreachable!()
                };
                let caller_instance = *instance;

                Some(
                    self.waitable_tables()[caller_instance]
                        .insert(guest_task.rep(), WaitableState::GuestTask)?,
                )
            } else {
                self.poll_for_result(guest_task)?;
                status = Status::Returned;
                None
            }
        } else {
            None
        };

        if let Some(storage) = storage {
            if let Some(result) = self.get_mut(guest_task)?.sync_result.take() {
                if let Some(result) = result {
                    storage[0] = MaybeUninit::new(result);
                }
            } else {
                return Err(anyhow!(crate::Trap::NoAsyncResult));
            }
        }
        Ok(status.pack(waitable))
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
        let wrapped = Arc::new(Mutex::new(AbortWrapper::Unpolled(Box::pin(future))));
        let task = self.push(HostTask::new(
            caller_instance,
            Some(AbortHandle::new(wrapped.clone())),
        ))?;
        let waitable = self.get(task)?.common.rep.clone();
        waitable.store(task.rep(), Relaxed);

        // Here we wrap the future in a `future::poll_fn` to ensure that we restore
        // and save the `CallContext` for this task before and after polling it,
        // respectively.  This involves unsafe shenanigans in order to smuggle the
        // store pointer into the wrapping future, alas.
        //
        // Note that we also wrap the future in order to provide cancellation
        // support via `AbortWrapper`.

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
            let store = VMStoreRawPtr(store.0.traitobj());
            let mut call_context = None;
            move |cx| {
                let mut wrapped = wrapped.try_lock().unwrap();

                let inner = mem::replace(DerefMut::deref_mut(&mut wrapped), AbortWrapper::Aborted);
                if let AbortWrapper::Unpolled(mut future)
                | AbortWrapper::Polled { mut future, .. } = inner
                {
                    maybe_push::<T>(store, &mut call_context, task);

                    let result = future.as_mut().poll(cx);

                    *DerefMut::deref_mut(&mut wrapped) = AbortWrapper::Polled {
                        future,
                        waker: cx.waker().clone(),
                    };

                    match result {
                        Poll::Ready(output) => Poll::Ready(Some(output)),
                        Poll::Pending => {
                            pop::<T>(store, &mut call_context, task);
                            Poll::Pending
                        }
                    }
                } else {
                    Poll::Ready(None)
                }
            }
        });

        log::trace!("new host task child of {}: {}", caller.rep(), task.rep());
        let mut future = Box::pin(future.map(move |result| {
            if let Some(result) = result {
                HostTaskOutput::Waitable {
                    waitable,
                    fun: Box::new(move |instance| {
                        let store = unsafe { StoreContextMut(&mut *instance.store().cast()) };
                        lower(store, result?)?;
                        instance.get_mut(task)?.abort_handle.take();
                        Ok(Event::Subtask {
                            status: Status::Returned,
                        })
                    }),
                }
            } else {
                // Task was cancelled, so we (ab)use the
                // `HostTaskOutput::Background` case to indicate there's nothing
                // more to be done.
                HostTaskOutput::Background(Ok(()))
            }
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
                    fun(self)?;
                    self.delete(task)?;
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
        let task = self.push(HostTask::new(caller_instance, None))?;
        let waitable = self.get(task)?.common.rep.clone();
        waitable.store(task.rep(), Relaxed);

        log::trace!("new host task child of {}: {}", caller.rep(), task.rep());
        let mut future = Box::pin(future.map(move |result| HostTaskOutput::Waitable {
            waitable,
            fun: Box::new(move |instance| {
                instance.get_mut(caller)?.result = Some(Box::new(result?) as _);
                Ok(Event::Subtask {
                    status: Status::Returned,
                })
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
        let instance = SendSyncPtr::new(NonNull::new(self).unwrap());
        let mut future = pin!(unsafe {
            on_fiber_raw(VMStoreRawPtr(store.traitobj()), None, move |_| {
                self.loop_until(future)
            })
        });
        future::poll_fn(move |cx| {
            let old = INSTANCE.with(|v| v.replace(instance.as_ptr()));
            let _reset_instance = ResetInstance(NonNull::new(old).map(SendSyncPtr::new));
            future.as_mut().poll(cx)
        })
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
            .ok_or_else(|| {
                anyhow!("`task.return` or `task.cancel` called more than once for current task")
            })?;

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

        self.task_complete(guest_task, result, Status::Returned)
    }

    pub(crate) fn task_cancel(
        &mut self,
        _caller_instance: RuntimeComponentInstanceIndex,
    ) -> Result<()> {
        let guest_task = self.guest_task().unwrap();
        let task = self.get_mut(guest_task)?;
        if !task.cancel_sent {
            bail!("`task.cancel` called by task which has not been cancelled")
        }
        _ = task.lift_result.take().ok_or_else(|| {
            anyhow!("`task.return` or `task.cancel` called more than once for current task")
        })?;

        assert!(task.result.is_none());

        log::trace!("task.cancel for {}", guest_task.rep());

        self.task_complete(guest_task, Box::new(DummyResult), Status::ReturnCancelled)
    }

    fn task_complete(
        &mut self,
        guest_task: TableId<GuestTask>,
        result: Box<dyn Any + Send + Sync>,
        status: Status,
    ) -> Result<()> {
        let (calls, host_table, _) = unsafe { &mut *self.store() }
            .store_opaque_mut()
            .component_resource_state();
        ResourceTables {
            calls,
            host_table: Some(host_table),
            tables: Some(self.component_resource_tables()),
        }
        .exit_call()?;

        let task = self.get_mut(guest_task)?;

        if let Caller::Host(tx) = &mut task.caller {
            if let Some(tx) = tx.take() {
                _ = tx.send(result);
            }
        } else {
            task.result = Some(result);
        }

        if let Caller::Guest { .. } = &task.caller {
            let waitable = Waitable::Guest(guest_task);
            waitable.set_event(self, Some(Event::Subtask { status }))?;
            waitable.send_or_mark_ready(self)?;
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

        if old && !new && !entry.pending.is_empty() {
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
                set.map(|set| {
                    Ok::<_, anyhow::Error>(
                        instance.get(set)?.ready.is_empty()
                            && instance.get(guest_task)?.event.is_none(),
                    )
                })
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
            WaitableCheck::Wait(params) | WaitableCheck::Poll(params) => {
                let event_and_handle = if let Some(waitable) =
                    self.get_mut(params.set)?.ready.pop_first()
                {
                    let event = waitable.common(self)?.event.take().unwrap();

                    log::trace!(
                            "deliver event {event:?} via waitable-set.{{wait,poll}} to {} for {}; set {}",
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

                    Some((event, handle))
                } else if let Some(event) = self.get_mut(guest_task)?.event.take() {
                    log::trace!(
                        "deliver event {event:?} via waitable-set.{{wait,poll}} to {}",
                        guest_task.rep()
                    );

                    Some((event, 0))
                } else {
                    None
                };

                let store_and_options = |me: &mut Self| unsafe {
                    let store = (*me.store()).store_opaque_mut();
                    let options = Options::new(
                        store.id(),
                        NonNull::new(params.memory),
                        None,
                        StringEncoding::Utf8,
                        true,
                        None,
                    );
                    (store, options)
                };

                if wait {
                    self.get_mut(params.set)?.waiting.remove(&guest_task);

                    let (event, handle) =
                        event_and_handle.ok_or_else(|| anyhow!("no waitables to wait for"))?;

                    let (ordinal, result) = event.parts();
                    let (store, options) = store_and_options(self);
                    let ptr = func::validate_inbounds::<(u32, u32)>(
                        options.memory_mut(store),
                        &ValRaw::u32(params.payload),
                    )?;
                    options.memory_mut(store)[ptr + 0..][..4]
                        .copy_from_slice(&handle.to_le_bytes());
                    options.memory_mut(store)[ptr + 4..][..4]
                        .copy_from_slice(&result.to_le_bytes());

                    Ok(ordinal)
                } else {
                    if let Some((event, handle)) = event_and_handle {
                        let (ordinal, result) = event.parts();
                        let (store, options) = store_and_options(self);
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

    pub(crate) fn subtask_cancel(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        task_id: u32,
    ) -> Result<u32> {
        let (rep, state) = self.waitable_tables()[caller_instance].get_mut_by_index(task_id)?;
        log::trace!("subtask_cancel {rep} (handle {task_id})");
        let (expected_caller_instance, host_task) = match state {
            WaitableState::HostTask => (
                self.get(TableId::<HostTask>::new(rep))?.caller_instance,
                true,
            ),
            WaitableState::GuestTask => {
                if let Caller::Guest { instance, .. } =
                    &self.get(TableId::<GuestTask>::new(rep))?.caller
                {
                    (*instance, false)
                } else {
                    unreachable!()
                }
            }
            _ => bail!("invalid task handle: {task_id}"),
        };
        assert_eq!(expected_caller_instance, caller_instance);

        let waitable = if host_task {
            let host_task = TableId::<HostTask>::new(rep);
            if let Some(handle) = self.get_mut(host_task)?.abort_handle.take() {
                handle.abort();
                return Ok(Status::ReturnCancelled as u32);
            }

            Waitable::Host(host_task)
        } else {
            let guest_task = TableId::<GuestTask>::new(rep);
            let task = self.get_mut(guest_task)?;
            if task.lower_params.is_some() {
                // Not yet started; cancel and remove from pending
                let callee_instance = task.instance;

                let was_present = self
                    .instance_states()
                    .get_mut(&callee_instance)
                    .unwrap()
                    .pending
                    .remove(&guest_task);

                if !was_present {
                    bail!("`subtask.cancel` called after terminal status delivered");
                }

                return Ok(Status::StartCancelled as u32);
            } else if task.lift_result.is_some() {
                // Started, but not yet returned or cancelled; send the
                // `CANCELLED` event
                task.cancel_sent = true;
                self.maybe_send_event(guest_task, Event::Cancelled, None)?;

                let task = self.get_mut(guest_task)?;
                if task.lift_result.is_some() {
                    // Still not yet returned or cancelled; if `async_`, return
                    // `BLOCKED`; otherwise wait
                    if async_ {
                        return Ok(BLOCKED);
                    } else {
                        let old_has_suspended = mem::replace(&mut task.has_suspended, false);
                        let caller = self.guest_task().unwrap();
                        let set = self.get_mut(caller)?.sync_call_set;
                        self.get_mut(set)?.waiting.insert(caller);
                        Waitable::Guest(guest_task).join(self, Some(set))?;
                        self.poll_for_result(guest_task)?;
                        self.get_mut(guest_task)?.has_suspended = old_has_suspended;
                    }
                }
            }

            Waitable::Guest(guest_task)
        };

        let event = waitable.take_event(self)?;
        if let Some(Event::Subtask {
            status: status @ (Status::Returned | Status::ReturnCancelled),
        }) = event
        {
            Ok(status as u32)
        } else {
            bail!("`subtask.cancel` called after terminal status delivered");
        }
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
    /// Poll the specified future as part of this instance's event loop until it
    /// yields a result _or_ there are no more tasks to run.
    ///
    /// This is intended for use in the top-level code of a host embedding with
    /// `Future`s which depend (directly or indirectly) on previously-started
    /// concurrent tasks: e.g. the `Future`s returned by
    /// `TypedFunc::call_concurrent`, `StreamReader::read`, etc., or `Future`s
    /// derived from those.
    ///
    /// Such `Future`s can only usefully be polled in the context of the event
    /// loop of the instance from which they originated; they will panic if
    /// polled elsewhere.  `Future`s returned by host functions registered using
    /// `LinkerInstance::func_wrap_concurrent` are always polled as part of the
    /// event loop, so this function is not needed in that case.  However,
    /// top-level code which needs to poll `Future`s involving concurrent tasks
    /// must use either this function, `Instance::with`, or `Instance::spawn` to
    /// ensure they are polled as part of the correct event loop.
    ///
    /// Consider the following examples:
    ///
    /// ```
    /// # use {
    /// #   anyhow::{anyhow, Result},
    /// #   wasmtime::{
    /// #     component::{ Component, Linker, HostFuture },
    /// #     Config, Engine, Store
    /// #   },
    /// # };
    /// #
    /// # async fn foo() -> Result<()> {
    /// # let mut config = Config::new();
    /// # let engine = Engine::new(&config)?;
    /// # let mut store = Store::new(&engine, ());
    /// # let mut linker = Linker::new(&engine);
    /// # let component = Component::new(&engine, "")?;
    /// linker.root().func_wrap_concurrent("foo", |accessor, (future,): (HostFuture<bool>,)| Box::pin(async move {
    ///     let future = accessor.with(|view| future.into_reader(view));
    ///     // We can `.await` this directly (i.e. without using
    ///     // `Instance::{run,run_with,spawn}`) since we're running in a host
    ///     // function:
    ///     Ok((future.read().await.ok_or_else(|| anyhow!("read failed"))?,))
    /// }))?;
    /// let instance = linker.instantiate_async(&mut store, &component).await?;
    /// let bar = instance.get_typed_func::<(), (HostFuture<bool>,)>(&mut store, "bar")?;
    /// let call = bar.call_concurrent(&mut store, ());
    ///
    /// // // NOT OK; this will panic if polled outside the event loop:
    /// // let (future,) = call.await?;
    ///
    /// // OK, since we use `Instance::run` to poll `call` inside the event loop:
    /// let (future,) = instance.run(&mut store, call).await??;
    ///
    /// let future = future.into_reader(&mut store);
    ///
    /// // // NOT OK; this will panic if polled outside the event loop:
    /// // let _result = future.read().await;
    ///
    /// // OK, since we use `Instance::run` to poll the `Future` returned by
    /// // `FutureReader::read`. Here we wrap that future in an async block for
    /// // illustration, although it's redundant for a simple case like this. In
    /// // a more complex scenario, we could use composition, loops, conditionals,
    /// // etc.
    /// let _result = instance.run(&mut store, Box::pin(async move {
    ///     future.read().await
    /// })).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run<U: Send, V: Send + Sync + 'static>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fut: impl Future<Output = V> + Send,
    ) -> Result<V> {
        check_recursive_run();
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[self.0].as_ref().unwrap().instance_ptr() };
        // TODO: can we avoid the `Box::pin` (which may be redundant if `fut` is
        // already a `Pin<Box<_>>`) here?
        instance.poll_until(store, Box::pin(fut)).await
    }

    /// Run the specified task as part of this instance's event loop.
    ///
    /// Like [`run`], this will poll a specified future as part of this
    /// instance's event loop until it yields a result _or_ there are no more
    /// tasks to run.  Unlike [`run`], the future may close over an
    /// [`Accessor`], which provides controlled access to the `Store` and its
    /// data.
    ///
    /// This enables a different control flow model than `run` in that the
    /// future has arbitrary access to the `Store` between `await` operations,
    /// whereas with `run` the future has no access to the `Store`.  Either one
    /// can be used to interleave `await` operations and `Store` access;
    /// i.e. you can either:
    ///
    /// - Call `run` multiple times with access to the `Store` in between,
    /// possibly moving resources, streams, etc. between the `Store` and the
    /// futures passed to `run`.
    ///
    /// ```
    /// # use {
    /// #   anyhow::Result,
    /// #   wasmtime::{
    /// #     component::{ Component, Linker, Resource, ResourceTable },
    /// #     Config, Engine, Store
    /// #   },
    /// # };
    /// #
    /// # struct MyResource(u32);
    /// # struct Ctx { table: ResourceTable }
    /// #
    /// # async fn foo() -> Result<()> {
    /// # let mut config = Config::new();
    /// # let engine = Engine::new(&config)?;
    /// # let mut store = Store::new(&engine, Ctx { table: ResourceTable::new() });
    /// # let mut linker = Linker::new(&engine);
    /// # let component = Component::new(&engine, "")?;
    /// # let instance = linker.instantiate_async(&mut store, &component).await?;
    /// # let foo = instance.get_typed_func::<(Resource<MyResource>,), (Resource<MyResource>,)>(&mut store, "foo")?;
    /// # let bar = instance.get_typed_func::<(u32,), ()>(&mut store, "bar")?;
    /// let resource = store.data_mut().table.push(MyResource(42))?;
    /// let call = foo.call_concurrent(&mut store, (resource,));
    /// let (another_resource,) = instance.run(&mut store, call).await??;
    /// let value = store.data_mut().table.delete(another_resource)?;
    /// let call = bar.call_concurrent(&mut store, (value.0,));
    /// instance.run(&mut store, call).await??;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// - Call `run_with` once and use `Accessor::with` to access the store from
    /// within the future.
    ///
    /// ```
    /// # use {
    /// #   anyhow::{Result, Error},
    /// #   wasmtime::{
    /// #     component::{ Component, Linker, Resource, ResourceTable, Accessor, Access },
    /// #     Config, Engine, Store
    /// #   },
    /// # };
    /// #
    /// # struct MyResource(u32);
    /// # struct Ctx { table: ResourceTable }
    /// #
    /// # async fn foo() -> Result<()> {
    /// # let mut config = Config::new();
    /// # let engine = Engine::new(&config)?;
    /// # let mut store = Store::new(&engine, Ctx { table: ResourceTable::new() });
    /// # let mut linker = Linker::new(&engine);
    /// # let component = Component::new(&engine, "")?;
    /// # let instance = linker.instantiate_async(&mut store, &component).await?;
    /// # let foo = instance.get_typed_func::<(Resource<MyResource>,), (Resource<MyResource>,)>(&mut store, "foo")?;
    /// # let bar = instance.get_typed_func::<(u32,), ()>(&mut store, "bar")?;
    /// instance.run_with(&mut store, move |accessor: &mut Accessor<_, _>| Box::pin(async move {
    ///    let (another_resource,) = accessor.with(|mut access: Access<Ctx, Ctx>| {
    ///        let resource = access.table.push(MyResource(42))?;
    ///        Ok::<_, Error>(foo.call_concurrent(access, (resource,)))
    ///    })?.await?;
    ///    accessor.with(|mut access: Access<Ctx, Ctx>| {
    ///        let value = access.table.delete(another_resource)?;
    ///        Ok::<_, Error>(bar.call_concurrent(access, (value.0,)))
    ///    })?.await
    /// })).await??;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run_with<U: Send, V: Send + Sync + 'static, F>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fun: F,
    ) -> Result<V>
    where
        F: for<'a> FnOnce(&'a mut Accessor<U, U>) -> Pin<Box<dyn Future<Output = V> + Send + 'a>>
            + Send
            + 'static,
    {
        check_recursive_run();
        let future = self.run_with_raw(&mut store, fun);
        self.run(store, future).await
    }

    fn run_with_raw<U: Send, V: Send + Sync + 'static, F>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fun: F,
    ) -> Pin<Box<dyn Future<Output = V> + Send + 'static>>
    where
        F: for<'a> FnOnce(&'a mut Accessor<U, U>) -> Pin<Box<dyn Future<Output = V> + Send + 'a>>
            + Send
            + 'static,
    {
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[self.0].as_ref().unwrap().instance_ptr() };

        let mut accessor =
            unsafe { Accessor::new(get_host_and_store, spawn_task, instance.instance()) };
        let mut future = Box::pin(async move { fun(&mut accessor).await });
        let store = VMStoreRawPtr(store.traitobj());
        let instance = SendSyncPtr::new(NonNull::new(instance).unwrap());
        let future = future::poll_fn(move |cx| {
            poll_with_state::<U, _>(store, instance, cx, future.as_mut())
        });

        unsafe {
            mem::transmute::<
                Pin<Box<dyn Future<Output = V> + Send>>,
                Pin<Box<dyn Future<Output = V> + Send + 'static>>,
            >(Box::pin(future))
        }
    }

    /// Spawn a background task to run as part of this instance's event loop.
    ///
    /// The task will receive an `&mut Accessor<T, U>` and run concurrently with
    /// any other tasks in progress for the instance.
    ///
    /// Note that the task will only make progress if and when the event loop
    /// for this instance is run.
    ///
    /// The returned [`SpawnHandle`] may be used to cancel the task.
    pub fn spawn<U: Send>(
        &self,
        store: impl AsContextMut<Data = U>,
        task: impl AccessorTask<U, U, Result<()>>,
    ) -> AbortHandle {
        let mut future = self.run_with_raw(store, move |accessor| {
            Box::pin(future::ready(accessor.spawn(task)))
        });
        match future
            .as_mut()
            .poll(&mut Context::from_waker(&dummy_waker()))
        {
            Poll::Ready(handle) => handle,
            Poll::Pending => unreachable!(),
        }
    }

    #[doc(hidden)]
    pub fn spawn_raw(
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
        callee_instance: RuntimeComponentInstanceIndex,
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
        callee: *mut VMFuncRef,
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
        callee_instance: RuntimeComponentInstanceIndex,
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
        callee: *mut VMFuncRef,
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
        callee_instance: RuntimeComponentInstanceIndex,
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
            callee_instance,
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
        callee: *mut VMFuncRef,
        param_count: u32,
        storage: *mut MaybeUninit<ValRaw>,
        storage_len: usize,
    ) -> Result<()> {
        instance
            .exit_call(
                StoreContextMut(self),
                callback,
                ptr::null_mut(),
                callee,
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
        callee_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        string_encoding: u8,
        params: u32,
        results: u32,
    ) -> Result<()> {
        instance.enter_call::<T>(
            start,
            return_,
            caller_instance,
            callee_instance,
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
        callee: *mut VMFuncRef,
        param_count: u32,
        result_count: u32,
        flags: u32,
    ) -> Result<u32> {
        instance.exit_call(
            StoreContextMut(self),
            callback,
            post_return,
            callee,
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
        instance
            .guest_write(
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
            .map(|result| result.encode())
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
        instance
            .guest_read(
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
            .map(|result| result.encode())
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
        instance
            .guest_write(
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
            .map(|result| result.encode())
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
        instance
            .guest_read(
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
            .map(|result| result.encode())
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
        instance
            .guest_write(
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
            .map(|result| result.encode())
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
        instance
            .guest_read(
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
            .map(|result| result.encode())
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
    abort_handle: Option<AbortHandle>,
}

impl HostTask {
    fn new(
        caller_instance: RuntimeComponentInstanceIndex,
        abort_handle: Option<AbortHandle>,
    ) -> Self {
        Self {
            common: WaitableCommon::default(),
            caller_instance,
            has_suspended: false,
            abort_handle,
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
    Caller(TableId<GuestTask>),
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
    call_context: Option<CallContext>,
    sync_result: Option<Option<ValRaw>>,
    should_yield: bool,
    has_suspended: bool,
    cancel_sent: bool,
    context: [u32; 2],
    subtasks: HashSet<TableId<GuestTask>>,
    sync_call_set: TableId<WaitableSet>,
    instance: RuntimeComponentInstanceIndex,
    event: Option<Event>,
}

impl GuestTask {
    fn new(
        instance: &mut ComponentInstance,
        lower_params: RawLower,
        lift_result: LiftResult,
        caller: Caller,
        callback: Option<Callback>,
        component_instance: RuntimeComponentInstanceIndex,
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
            call_context: Some(CallContext::default()),
            sync_result: None,
            should_yield: false,
            has_suspended: false,
            cancel_sent: false,
            context: [0u32; 2],
            subtasks: HashSet::new(),
            sync_call_set,
            instance: component_instance,
            event: None,
        })
    }

    fn dispose(self, instance: &mut ComponentInstance, me: TableId<GuestTask>) -> Result<()> {
        instance.yielding().remove(&me);

        for waitable in mem::take(&mut instance.get_mut(self.sync_call_set)?.ready) {
            if let Some(Event::Subtask {
                status: Status::Returned | Status::ReturnCancelled,
            }) = waitable.common(instance)?.event
            {
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

    fn set_event(&self, instance: &mut ComponentInstance, event: Option<Event>) -> Result<()> {
        self.common(instance)?.event = event;
        Ok(())
    }

    fn take_event(&self, instance: &mut ComponentInstance) -> Result<Option<Event>> {
        let common = self.common(instance)?;
        let event = common.event.take();
        if let Some(set) = self.common(instance)?.set {
            instance.get_mut(set)?.ready.remove(self);
        }
        Ok(event)
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
    pending: BTreeSet<TableId<GuestTask>>,
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
    deferred: Vec<Box<dyn FnOnce(&mut ComponentInstance) -> Result<()> + Send + Sync + 'static>>,

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
            deferred: Vec::new(),
        }
    }

    pub fn drop_fibers(&mut self) {
        self.table = Table::new();
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

fn checked<F: Future + Send + 'static>(
    instance: SendSyncPtr<ComponentInstance>,
    fut: F,
) -> impl Future<Output = F::Output> + Send + 'static {
    let mut fut = Box::pin(fut);
    future::poll_fn(move |cx| {
        let message = "\
            `Future`s which depend on asynchronous component tasks, streams, or \
            futures to complete may only be polled from the event loop of the \
            instance from which they originated.  Please use \
            `Instance::{run,run_with,spawn}` to poll or await them.\
        ";
        INSTANCE.with(|v| {
            if v.get() != instance.as_ptr() {
                panic!("{message}")
            }
        });
        fut.as_mut().poll(cx)
    })
}

fn check_recursive_run() {
    INSTANCE.with(|v| {
        if !v.get().is_null() {
            panic!("Recursive `Instance::run{{_with}}` calls not supported")
        }
    });
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

#[cfg(feature = "gc")]
pub(crate) async fn on_fiber_opaque<R: Send + 'static>(
    store: &mut StoreOpaque,
    instance: Option<RuntimeComponentInstanceIndex>,
    func: impl FnOnce(&mut StoreOpaque) -> R + Send,
) -> Result<R> {
    unsafe {
        on_fiber_raw(VMStoreRawPtr(store.traitobj()), instance, move |store| {
            func((*store).store_opaque_mut())
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
        let mut storage = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
        let lower = instance.get_mut(guest_task)?.lower_params.take().unwrap();
        if !instance.may_enter(guest_task) {
            bail!(crate::Trap::CannotEnterComponent);
        }

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

pub(crate) struct PreparedCall<R> {
    handle: Func,
    task: TableId<GuestTask>,
    result_count: usize,
    rx: oneshot::Receiver<LiftedResult>,
    _phantom: PhantomData<R>,
}

pub(crate) fn prepare_call<T: Send, LowerParams: Copy, R>(
    store: StoreContextMut<T>,
    lower_params: LowerFn,
    lower_context: LiftLowerContext,
    lift_result: LiftFn,
    lift_context: LiftLowerContext,
    handle: Func,
) -> Result<PreparedCall<R>> {
    let func_data = &store.0[handle.0];
    let task_return_type = func_data.types[func_data.ty].results;
    let component_instance = func_data.component_instance;
    let instance = func_data.instance;
    let callback = func_data.options.callback;
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
        }),
        component_instance,
    )?;

    let task = instance.push(task)?;
    instance
        .get_mut(task)?
        .common
        .rep
        .store(task.rep(), Relaxed);

    Ok(PreparedCall {
        handle,
        task,
        result_count: mem::size_of::<LowerParams>() / mem::size_of::<ValRaw>(),
        rx,
        _phantom: PhantomData,
    })
}

pub(crate) fn defer_call<T, R: Send + 'static>(
    store: StoreContextMut<T>,
    prepared: PreparedCall<R>,
) -> impl Future<Output = Result<R>> + Send + 'static + use<T, R> {
    let PreparedCall {
        handle,
        task,
        result_count,
        rx,
        ..
    } = prepared;
    let func_data = &store[handle.0];
    let instance = func_data.instance;
    let instance_ptr = store.0[instance.0].as_ref().unwrap().instance_ptr();
    let instance = unsafe { &mut *instance_ptr };
    instance
        .concurrent_state
        .deferred
        .push(Box::new(move |instance: &mut ComponentInstance| {
            start_call(
                unsafe { StoreContextMut::<T>(&mut *instance.store().cast()) },
                handle,
                task,
                result_count,
            )
        }));

    checked(
        SendSyncPtr::new(NonNull::new(instance_ptr).unwrap()),
        rx.map(|result| {
            result
                .map(|v| *v.downcast().unwrap())
                .map_err(anyhow::Error::from)
        }),
    )
}

fn start_call<T>(
    mut store: StoreContextMut<T>,
    handle: Func,
    guest_task: TableId<GuestTask>,
    result_count: usize,
) -> Result<()> {
    let func_data = &store.0[handle.0];
    let is_concurrent = func_data.options.async_();
    let component_instance = func_data.component_instance;
    let instance = func_data.instance;
    let callee = func_data.export.func_ref;
    let callback = func_data.options.callback;
    let post_return = func_data.post_return;

    let instance = unsafe { &mut *store.0[instance.0].as_ref().unwrap().instance_ptr() };

    log::trace!("starting call {}", guest_task.rep());

    let call = make_call(
        guest_task,
        SendSyncPtr::new(callee),
        result_count,
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
        1,
        false,
    )?;

    *instance.guest_task() = None;
    log::trace!("popped current task {}; new task is None", guest_task.rep());

    log::trace!("started call {}", guest_task.rep());

    Ok(())
}

pub(crate) fn call<T: Send, LowerParams: Copy, R: Send + Sync + 'static>(
    mut store: StoreContextMut<T>,
    lower_params: LowerFn,
    lower_context: LiftLowerContext,
    lift_result: LiftFn,
    lift_context: LiftLowerContext,
    handle: Func,
) -> Result<R> {
    let prepared = prepare_call::<_, LowerParams, R>(
        store.as_context_mut(),
        lower_params,
        lower_context,
        lift_result,
        lift_context,
        handle,
    )?;

    start_call(
        store.as_context_mut(),
        prepared.handle,
        prepared.task,
        prepared.result_count,
    )?;

    let future = prepared
        .rx
        .map(|result| *result.unwrap().downcast().unwrap());

    let instance = unsafe {
        &mut *store.0[store.0[handle.0].instance.0]
            .as_ref()
            .unwrap()
            .instance_ptr()
    };
    instance.loop_until(Box::pin(future))
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
