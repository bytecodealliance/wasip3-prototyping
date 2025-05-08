use {
    crate::{
        component::{
            func::{self, Func, Options},
            HasData, HasSelf, Instance,
        },
        store::{StoreInner, StoreOpaque, StoreToken},
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
        future::{self, Either, FutureExt},
        stream::{FuturesUnordered, StreamExt},
    },
    futures_and_streams::{FlatAbi, ReturnCode, StreamFutureState, TableIndex, TransmitHandle},
    once_cell::sync::Lazy,
    states::StateTable,
    std::{
        any::Any,
        borrow::ToOwned,
        boxed::Box,
        cell::{Cell, RefCell, UnsafeCell},
        collections::{BTreeMap, BTreeSet, HashMap, HashSet},
        fmt,
        future::Future,
        marker::PhantomData,
        mem::{self, MaybeUninit},
        ops::{DerefMut, Range},
        pin::{pin, Pin},
        ptr::{self, NonNull},
        sync::{
            atomic::{AtomicPtr, Ordering::Relaxed},
            Arc, Mutex,
        },
        task::{Context, Poll, Wake, Waker},
        vec::Vec,
    },
    table::{Table, TableDebug, TableError, TableId},
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
pub struct Access<'a, T, D: HasData = HasSelf<T>>(&'a mut Accessor<T, D>);

impl<'a, T, D> Access<'a, T, D>
where
    D: HasData,
{
    /// Get mutable access to the store data.
    pub fn data_mut(&mut self) -> &mut T {
        self.as_context_mut().0.data_mut()
    }

    /// Get mutable access to the store data.
    pub fn get(&mut self) -> D::Data<'_> {
        let get_data = self.0.get_data;
        get_data(self.data_mut())
    }

    /// Spawn a background task.
    ///
    /// See [`Accessor::spawn`] for details.
    pub fn spawn(&mut self, task: impl AccessorTask<T, D, Result<()>>) -> AbortHandle
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

impl<'a, T, D> AsContext for Access<'a, T, D>
where
    D: HasData,
{
    type Data = T;

    fn as_context(&self) -> StoreContext<T> {
        // SAFETY: TODO
        unsafe { StoreContext(&*(self.0.get)().cast()) }
    }
}

impl<'a, T, D> AsContextMut for Access<'a, T, D>
where
    D: HasData,
{
    fn as_context_mut(&mut self) -> StoreContextMut<T> {
        // SAFETY: TODO
        unsafe { StoreContextMut(&mut *(self.0.get)().cast()) }
    }
}

/// Provides scoped mutable access to store data in the context of a concurrent
/// host import function.
///
/// This allows multiple host import futures to execute concurrently and access
/// the store data between (but not across) `await` points.
pub struct Accessor<T, D = HasSelf<T>>
where
    D: HasData,
{
    get: Arc<dyn Fn() -> *mut u8 + Send + Sync>,
    get_data: fn(&mut T) -> D::Data<'_>,
    spawn: fn(Spawned),
    instance: Option<Instance>,
    _phantom: PhantomData<fn() -> *mut StoreInner<T>>,
}

impl<T, D> Accessor<T, D>
where
    D: HasData,
{
    /// Creates a new `Accessor` backed by the specified functions.
    ///
    /// - `get`: used to retrieve the host data and store
    ///
    /// - `spawn`: used to queue spawned background tasks to be run later
    ///
    /// - `instance`: used to access the `Instance` to which this `Accessor`
    /// (and the future which closes over it) belongs
    ///
    /// SAFETY: This relies on `get` either returning a pair of pointers such
    /// that the first is a valid `*mut U` and the second is a `*mut dyn VMStore`
    /// whose data is of type `T` _or_ panicking if it is called outside its
    /// intended scope.  See the comment in `Access::get` for further details.
    #[doc(hidden)]
    pub unsafe fn new(
        get: fn() -> *mut u8,
        get_data: fn(&mut T) -> D::Data<'_>,
        spawn: fn(Spawned),
        instance: Option<Instance>,
    ) -> Self {
        Self {
            get: Arc::new(get),
            get_data,
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
    pub fn with<R>(&mut self, fun: impl FnOnce(Access<'_, T, D>) -> R) -> R {
        fun(Access(self))
    }

    /// TODO: is this safe? unsafe? should there be a lifetime in the
    /// returned value? no?
    #[doc(hidden)]
    pub unsafe fn with_data<D2: HasData>(
        &mut self,
        get_data: fn(&mut T) -> D2::Data<'_>,
    ) -> Accessor<T, D2> {
        Accessor {
            get: self.get.clone(),
            get_data,
            spawn: self.spawn,
            instance: self.instance,
            _phantom: PhantomData,
        }
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
    pub fn spawn(&mut self, task: impl AccessorTask<T, D, Result<()>>) -> AbortHandle {
        let mut accessor = Self {
            get: self.get.clone(),
            get_data: self.get_data,
            spawn: self.spawn,
            instance: self.instance,
            _phantom: PhantomData,
        };
        let future = Arc::new(Mutex::new(AbortWrapper::Unpolled(unsafe {
            // This `transmute` is to avoid requiring a `U: 'static` bound,
            // which should be unnecessary.
            //
            // SAFETY: We don't actually store a value of type `U` in the
            // `Accessor` we're `move`ing into the `async` block; access to a
            // `U` is brokered via `Accessor::with` by way of a thread-local
            // variable in `wasmtime-wit-bindgen`-generated code or the
            // `poll_with_state` function in this module.  Furthermore,
            // `AccessorTask` implementations are required to be `'static`, so
            // no lifetime issues there.  We have no way to explain any of that
            // to the compiler, though, so we resort to a transmute here.
            //
            // See the comment in `Access::get` for further details.
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
pub trait AccessorTask<T, D, R>: Send + 'static
where
    D: HasData,
{
    /// Run the task.
    fn run(self, accessor: &mut Accessor<T, D>) -> impl Future<Output = R> + Send;
}

struct State {
    store: *mut u8,
    spawned: Vec<Spawned>,
}

#[derive(Copy, Clone)]
enum InstanceThreadLocalState {
    None,
    Polling,
    Detached {
        instance: SendSyncPtr<ComponentInstance>,
        store: VMStoreRawPtr,
    },
    Attached {
        instance: Instance,
    },
}

impl fmt::Debug for InstanceThreadLocalState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(match self {
            Self::None => "None",
            Self::Polling => "Polling",
            Self::Detached { .. } => "Detached",
            Self::Attached { .. } => "Attached",
        })
        .finish()
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = RefCell::new(None);
    static INSTANCE_STATE: Cell<InstanceThreadLocalState> = Cell::new(InstanceThreadLocalState::None);
}

fn with_local_instance<R>(fun: impl FnOnce(&mut dyn VMStore, &mut ComponentInstance) -> R) -> R {
    let state = ResetInstanceThreadLocalState(
        INSTANCE_STATE.with(|v| v.replace(InstanceThreadLocalState::Polling)),
    );

    let InstanceThreadLocalState::Detached { instance, store } = state.0 else {
        unreachable!("expected `Detached`; got `{:?}`", state.0)
    };
    let (store, instance) = unsafe { (&mut *store.0.as_ptr(), &mut *instance.as_ptr()) };
    fun(store, instance)
}

unsafe fn poll_with_local_instance<F: Future + Send + ?Sized>(
    store: VMStoreRawPtr,
    instance: SendSyncPtr<ComponentInstance>,
    future: &mut Pin<&mut F>,
    cx: &mut Context,
) -> Poll<F::Output> {
    let state = ResetInstanceThreadLocalState(
        INSTANCE_STATE.with(|v| v.replace(InstanceThreadLocalState::Detached { instance, store })),
    );

    assert!(matches!(state.0, InstanceThreadLocalState::None));

    future.as_mut().poll(cx)
}

struct ResetState(Option<State>);

impl Drop for ResetState {
    fn drop(&mut self) {
        STATE.with(|v| {
            *v.borrow_mut() = self.0.take();
        })
    }
}

struct ResetInstanceThreadLocalState(InstanceThreadLocalState);

impl Drop for ResetInstanceThreadLocalState {
    fn drop(&mut self) {
        INSTANCE_STATE.with(|v| v.set(self.0))
    }
}

fn get_store() -> *mut u8 {
    STATE
        .with(|v| v.borrow().as_ref().map(|State { store, .. }| *store))
        .unwrap()
}

fn spawn_task(task: Spawned) {
    STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
}

// SAFETY: `store` must be a valid `*mut dyn VMStore` with a data type of `T`,
// and `instance` must be a valid `*mut ComponentInstance`.
//
// Note that we must smuggle these pointers in as `VMStoreRawPtr` and
// `SendSyncPtr<ComponentInstance>`, respectively, to allow this function to be
// called within a future that is `Send`.  This is sound because
// `ComponentInstance::poll_until` is the only place those futures are polled,
// that function has exclusive access to both the store and the
// `ComponentInstance`.
fn poll_with_state<T: 'static, F: Future + ?Sized>(
    token: StoreToken<T>,
    cx: &mut Context,
    future: Pin<&mut F>,
) -> Poll<F::Output> {
    with_local_instance(|store, instance| {
        let store_ptr = (store as *mut dyn VMStore).cast();
        let mut store_cx = token.as_context_mut(store);

        let (result, spawned) = store_cx.with_attached_instance(instance, |_, _| {
            let old_state = STATE.with(|v| {
                v.replace(Some(State {
                    store: store_ptr,
                    spawned: Vec::new(),
                }))
            });
            let _reset_state = ResetState(old_state);
            (future.poll(cx), STATE.with(|v| v.take()).unwrap().spawned)
        });

        for spawned in spawned {
            instance.spawn(future::poll_fn(move |cx| {
                let mut spawned = spawned.try_lock().unwrap();
                let inner = mem::replace(DerefMut::deref_mut(&mut spawned), AbortWrapper::Aborted);
                if let AbortWrapper::Unpolled(mut future)
                | AbortWrapper::Polled { mut future, .. } = inner
                {
                    let result = poll_with_state::<T, _>(token, cx, future.as_mut());
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
    })
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

enum WaitMode {
    Fiber(StoreFiber<'static>),
    Callback(RuntimeComponentInstanceIndex),
}

#[derive(Debug)]
enum SuspendReason {
    Waiting {
        set: TableId<WaitableSet>,
        task: TableId<GuestTask>,
    },
    NeedWork,
    Yielding {
        task: TableId<GuestTask>,
    },
}

enum GuestCallKind {
    DeliverEvent {
        instance: RuntimeComponentInstanceIndex,
        set: Option<TableId<WaitableSet>>,
    },
    Start(Box<dyn FnOnce(&mut dyn VMStore, &mut ComponentInstance) -> Result<()> + Send + Sync>),
}

impl fmt::Debug for GuestCallKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::DeliverEvent { instance, set } => f
                .debug_struct("DeliverEvent")
                .field("instance", instance)
                .field("set", set)
                .finish(),
            Self::Start(_) => f.debug_tuple("Start").finish(),
        }
    }
}

#[derive(Debug)]
struct GuestCall {
    task: TableId<GuestTask>,
    kind: GuestCallKind,
}

impl GuestCall {
    fn is_ready(&self, instance: &mut ComponentInstance) -> Result<bool> {
        let task_instance = instance.get(self.task)?.instance;
        let state = instance.instance_state(task_instance);
        let ready = match &self.kind {
            GuestCallKind::DeliverEvent { .. } => !state.do_not_enter,
            GuestCallKind::Start(_) => !(state.do_not_enter || state.backpressure),
        };
        log::trace!(
            "call {self:?} ready? {ready} (do_not_enter: {}; backpressure: {})",
            state.do_not_enter,
            state.backpressure
        );
        Ok(ready)
    }
}

#[derive(Debug)]
struct PollParams {
    task: TableId<GuestTask>,
    set: TableId<WaitableSet>,
    instance: RuntimeComponentInstanceIndex,
}

enum WorkItem {
    PushFuture(Mutex<HostTaskFuture>),
    ResumeFiber(StoreFiber<'static>),
    GuestCall(GuestCall),
    Poll(PollParams),
}

impl fmt::Debug for WorkItem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::PushFuture(_) => f.debug_tuple("PushFuture").finish(),
            Self::ResumeFiber(_) => f.debug_tuple("ResumeFiber").finish(),
            Self::GuestCall(call) => f.debug_tuple("GuestCall").field(call).finish(),
            Self::Poll(params) => f.debug_tuple("Poll").field(params).finish(),
        }
    }
}

impl<T> StoreContextMut<'_, T> {
    fn with_detached_instance<R>(
        &mut self,
        instance: &Instance,
        fun: impl FnOnce(StoreContextMut<'_, T>, &mut ComponentInstance) -> R,
    ) -> R {
        let _state = ResetInstanceThreadLocalState(INSTANCE_STATE.with(|v| match v.get() {
            state @ (InstanceThreadLocalState::None | InstanceThreadLocalState::Polling) => state,
            InstanceThreadLocalState::Attached { .. } => {
                v.replace(InstanceThreadLocalState::Polling)
            }
            _ => unreachable!(),
        }));
        let data = self.0[instance.0].take().unwrap();
        let (result, data) = {
            // SAFETY: We've taken the instance out of the store, so now we own
            // it and can take an exclusive reference to it.
            let instance = unsafe { &mut *data.instance_ptr() };
            assert!(instance.data.is_none());
            instance.data = Some(data);
            instance.set_store(None);
            let result = fun(self.as_context_mut(), instance);
            instance.set_store(Some(VMStoreRawPtr(self.traitobj())));
            (result, instance.data.take())
        };
        if self.0[instance.0].is_none() {
            self.0[instance.0] = data;
        }
        result
    }

    async fn with_detached_instance_async<R>(
        &mut self,
        instance: &Instance,
        fun: impl AsyncFnOnce(StoreContextMut<'_, T>, &mut ComponentInstance) -> R,
    ) -> R {
        let _state = ResetInstanceThreadLocalState(INSTANCE_STATE.with(|v| match v.get() {
            state @ (InstanceThreadLocalState::None | InstanceThreadLocalState::Polling) => state,
            InstanceThreadLocalState::Attached { .. } => {
                v.replace(InstanceThreadLocalState::Polling)
            }
            _ => unreachable!(),
        }));
        let data = self.0[instance.0].take().unwrap();
        let (result, data) = {
            // SAFETY: We've taken the instance out of the store, so now we own
            // it and can take an exclusive reference to it.
            let instance = unsafe { &mut *data.instance_ptr() };
            assert!(instance.data.is_none());
            instance.data = Some(data);
            instance.set_store(None);
            let result = fun(self.as_context_mut(), instance).await;
            instance.set_store(Some(VMStoreRawPtr(self.traitobj())));
            (result, instance.data.take())
        };
        if self.0[instance.0].is_none() {
            self.0[instance.0] = data;
        }
        result
    }

    pub(crate) fn with_attached_instance<R>(
        &mut self,
        instance: &mut ComponentInstance,
        fun: impl FnOnce(StoreContextMut<'_, T>, Option<Instance>) -> R,
    ) -> R {
        let _state = ResetInstanceThreadLocalState(INSTANCE_STATE.with(|v| match v.get() {
            state @ InstanceThreadLocalState::None => state,
            state @ InstanceThreadLocalState::Polling => {
                if let Some(handle) = instance.instance {
                    v.replace(InstanceThreadLocalState::Attached { instance: handle })
                } else {
                    state
                }
            }
            _ => unreachable!(),
        }));
        if let Some(handle) = instance.instance {
            self.0[handle.0] = Some(instance.data.take().unwrap());
        }
        instance.set_store(Some(VMStoreRawPtr(self.traitobj())));
        let result = fun(self.as_context_mut(), instance.instance);
        instance.set_store(None);
        if let Some(handle) = instance.instance {
            instance.data = Some(self.0[handle.0].take().unwrap());
        }
        result
    }
}

impl ComponentInstance {
    pub(crate) fn instance(&self) -> Option<Instance> {
        self.instance
    }

    fn instance_state(&mut self, instance: RuntimeComponentInstanceIndex) -> &mut InstanceState {
        self.concurrent_state
            .instance_states
            .entry(instance)
            .or_default()
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
        // Note that we can't just call
        // `self.concurrent_state.futures.get_mut().push(future)` here since
        // this may be called from a future that's being polled inside
        // `Self::poll_until`.  Normally, the borrow checker would statically
        // prevent that, but we're smuggling `*mut ComponentInstance` pointers
        // behind its back, so we need to ensure soundness ourselves.
        // Therefore, we push an item to the "high priority" queue, which will
        // actually push to `ConcurrentState::futures` later.
        self.push_high_priority(WorkItem::PushFuture(Mutex::new(future)));
    }

    pub(crate) fn spawn(&mut self, task: impl Future<Output = Result<()>> + Send + 'static) {
        self.push_future(Box::pin(task.map(HostTaskOutput::Result)))
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

    fn push_high_priority(&mut self, item: WorkItem) {
        log::trace!("push high priority: {item:?}");
        self.concurrent_state.high_priority.push(item);
    }

    fn push_low_priority(&mut self, item: WorkItem) {
        log::trace!("push low priority: {item:?}");
        self.concurrent_state.low_priority.push(item);
    }

    fn suspend_reason(&mut self) -> &mut Option<SuspendReason> {
        &mut self.concurrent_state.suspend_reason
    }

    fn worker(&mut self) -> &mut Option<StoreFiber<'static>> {
        &mut self.concurrent_state.worker
    }

    fn guest_call(&mut self) -> &mut Option<GuestCall> {
        &mut self.concurrent_state.guest_call
    }

    fn maybe_push_call_context(
        &mut self,
        store: &mut StoreOpaque,
        guest_task: TableId<GuestTask>,
    ) -> Result<()> {
        let task = self.get_mut(guest_task)?;
        if task.lift_result.is_some() {
            log::trace!("push call context for {guest_task:?}");
            let call_context = task.call_context.take().unwrap();
            store.component_resource_state().0.push(call_context);
        }
        Ok(())
    }

    fn maybe_pop_call_context(
        &mut self,
        store: &mut StoreOpaque,
        guest_task: TableId<GuestTask>,
    ) -> Result<()> {
        if self.get_mut(guest_task)?.lift_result.is_some() {
            log::trace!("pop call context for {guest_task:?}");
            let call_context = Some(store.component_resource_state().0.pop().unwrap());
            self.get_mut(guest_task)?.call_context = call_context;
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

    fn handle_callback_code(
        &mut self,
        guest_task: TableId<GuestTask>,
        runtime_instance: RuntimeComponentInstanceIndex,
        code: u32,
        pending_event: Event,
    ) -> Result<()> {
        let (code, set) = unpack_callback_code(code);

        log::trace!("received callback code from {guest_task:?}: {code} (set: {set})");

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
            Waitable::Guest(guest_task).set_event(self, Some(event))?;
        }

        match code {
            callback_code::EXIT => match &self.get(guest_task)?.caller {
                Caller::Host(_) => {
                    log::trace!("handle_callback_code will delete task {guest_task:?}");
                    Waitable::Guest(guest_task).delete_from(self)?;
                }
                Caller::Guest { .. } => {
                    self.get_mut(guest_task)?.callback = None;
                }
            },
            callback_code::YIELD => {
                let task = self.get_mut(guest_task)?;
                assert!(task.event.is_none());
                task.event = Some(Event::None);
                self.push_low_priority(WorkItem::GuestCall(GuestCall {
                    task: guest_task,
                    kind: GuestCallKind::DeliverEvent {
                        instance: runtime_instance,
                        set: None,
                    },
                }));
            }
            callback_code::WAIT | callback_code::POLL => {
                let set = get_set(self, set)?;

                if self.get_mut(guest_task)?.event.is_some() || !self.get_mut(set)?.ready.is_empty()
                {
                    self.push_high_priority(WorkItem::GuestCall(GuestCall {
                        task: guest_task,
                        kind: GuestCallKind::DeliverEvent {
                            instance: runtime_instance,
                            set: Some(set),
                        },
                    }));
                } else {
                    match code {
                        callback_code::POLL => {
                            self.push_low_priority(WorkItem::Poll(PollParams {
                                task: guest_task,
                                instance: runtime_instance,
                                set,
                            }));
                        }
                        callback_code::WAIT => {
                            let old = self.get_mut(guest_task)?.waiting_on.replace(set);
                            assert!(old.is_none());
                            let old = self
                                .get_mut(set)?
                                .waiting
                                .insert(guest_task, WaitMode::Callback(runtime_instance));
                            assert!(old.is_none());
                        }
                        _ => unreachable!(),
                    }
                }
            }
            _ => bail!("unsupported callback code: {code}"),
        }

        Ok(())
    }

    fn enter_instance(&mut self, instance: RuntimeComponentInstanceIndex) {
        self.instance_state(instance).do_not_enter = true;
    }

    fn exit_instance(&mut self, instance: RuntimeComponentInstanceIndex) -> Result<()> {
        self.instance_state(instance).do_not_enter = false;
        self.partition_pending(instance)
    }

    fn partition_pending(&mut self, instance: RuntimeComponentInstanceIndex) -> Result<()> {
        for (task, kind) in mem::take(&mut self.instance_state(instance).pending).into_iter() {
            let call = GuestCall { task, kind };
            if call.is_ready(self)? {
                self.push_high_priority(WorkItem::GuestCall(call));
            } else {
                self.instance_state(instance)
                    .pending
                    .insert(call.task, call.kind);
            }
        }

        Ok(())
    }

    /// Add the specified guest call to the `Self::high_priority` queue, to be
    /// started as soon as backpressure and/or reentrance rules allow.
    fn queue_call<T: 'static>(
        &mut self,
        mut store: StoreContextMut<T>,
        guest_task: TableId<GuestTask>,
        callee: SendSyncPtr<VMFuncRef>,
        param_count: usize,
        result_count: usize,
        flags: Option<InstanceFlags>,
        async_: bool,
        callback: Option<SendSyncPtr<VMFuncRef>>,
        post_return: Option<SendSyncPtr<VMFuncRef>>,
    ) -> Result<()> {
        // TODO: This function should create and queue `impl UnsafeFnOnce`
        // "closures" (where `UnsafeFnOnce` would need to be a trait we define
        // ourselves, since there's no standard equivalent) rather than `impl
        // FnOnce` closures.  That would force the caller to use an unsafe block
        // and (hopefully) uphold the contract we've described above.

        /// Return a closure which will call the specified function in the scope
        /// of the specified task.
        ///
        /// This will use `GuestTask::lower_params` to lower the parameters, but
        /// will not lift the result; instead, it returns a
        /// `[MaybeUninit<ValRaw>; MAX_FLAT_PARAMS]` from which the result, if
        /// any, may be lifted.  Note that an async-lifted export will have
        /// returned its result using the `task.return` intrinsic (or not
        /// returned a result at all, in the case of `task.cancel`), in which
        /// case the "result" of this call will either be a callback code or
        /// nothing.
        ///
        /// SAFETY: `callee` must be a valid `*mut VMFuncRef` at the time when
        /// the returned closure is called.  In addition the caller must confer
        /// exclusive access to the store to which the passed `&mut
        /// ComponentInstance` belongs, which must have a data type parameter of
        /// `T`.
        fn make_call<T: 'static>(
            store: StoreContextMut<T>,
            guest_task: TableId<GuestTask>,
            callee: SendSyncPtr<VMFuncRef>,
            param_count: usize,
            result_count: usize,
            flags: Option<InstanceFlags>,
        ) -> impl FnOnce(
            &mut dyn VMStore,
            &mut ComponentInstance,
        ) -> Result<[MaybeUninit<ValRaw>; MAX_FLAT_PARAMS]>
               + Send
               + Sync
               + 'static
               + use<T> {
            let token = StoreToken::new(store);
            move |store: &mut dyn VMStore, instance: &mut ComponentInstance| {
                let mut storage = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
                let lower = instance.get_mut(guest_task)?.lower_params.take().unwrap();

                lower(store, instance, &mut storage[..param_count])?;

                let mut store = token.as_context_mut(store);

                // SAFETY: Per the contract documented above, `callee` is a
                // valid pointer and `store` has a data type parameter of `T`.
                store.with_attached_instance(instance, |mut store, _| unsafe {
                    if let Some(mut flags) = flags {
                        flags.set_may_enter(false);
                    }
                    crate::Func::call_unchecked_raw(
                        &mut store,
                        callee.as_non_null(),
                        NonNull::new(
                            &mut storage[..param_count.max(result_count)]
                                as *mut [MaybeUninit<ValRaw>] as _,
                        )
                        .unwrap(),
                    )?;
                    if let Some(mut flags) = flags {
                        flags.set_may_enter(true);
                    }
                    Ok::<_, anyhow::Error>(())
                })?;

                Ok(storage)
            }
        }

        let call = make_call(
            store.as_context_mut(),
            guest_task,
            callee,
            param_count,
            result_count,
            flags,
        );

        let callee_instance = self.get(guest_task)?.instance;
        let fun = if callback.is_some() {
            assert!(async_);

            Box::new(
                move |store: &mut dyn VMStore, instance: &mut ComponentInstance| {
                    let old_task = instance.guest_task().replace(guest_task);
                    log::trace!(
                        "stackless call: replaced {old_task:?} with {guest_task:?} as current task"
                    );

                    instance.maybe_push_call_context(store.store_opaque_mut(), guest_task)?;

                    instance.enter_instance(callee_instance);

                    // SAFETY: See the documentation for `make_call` to review the
                    // contract we must uphold for `call` here.
                    //
                    // Per the contract described in the `queue_call` documentation,
                    // we can rely on exclusive access to the store, whose data type
                    // parameter is `T`.
                    let storage = call(store, instance)?;

                    instance.exit_instance(callee_instance)?;

                    instance.maybe_pop_call_context(store.store_opaque_mut(), guest_task)?;

                    *instance.guest_task() = old_task;
                    log::trace!("stackless call: restored {old_task:?} as current task");

                    // SAFETY: `wasmparser` will have validated that the callback
                    // function returns a `i32` result.
                    let code = unsafe { storage[0].assume_init() }.get_i32() as u32;

                    instance.handle_callback_code(
                        guest_task,
                        callee_instance,
                        code,
                        Event::Subtask {
                            status: Status::Started,
                        },
                    )?;

                    Ok(())
                },
            )
                as Box<
                    dyn FnOnce(&mut dyn VMStore, &mut ComponentInstance) -> Result<()>
                        + Send
                        + Sync,
                >
        } else {
            let token = StoreToken::new(store);
            Box::new(
                move |store: &mut dyn VMStore, instance: &mut ComponentInstance| {
                    let old_task = instance.guest_task().replace(guest_task);
                    log::trace!(
                        "stackful call: replaced {old_task:?} with {guest_task:?} as current task",
                    );

                    let mut flags = instance.instance_flags(callee_instance);

                    instance.maybe_push_call_context(store.store_opaque_mut(), guest_task)?;

                    if !async_ {
                        instance.enter_instance(callee_instance);
                    }

                    // SAFETY: Per the contract described in the `queue_call`
                    // documentation, we can rely on exclusive access to the store,
                    // whose data type parameter is `T`.
                    let storage = call(store, instance)?;

                    if async_ {
                        if instance.get(guest_task)?.lift_result.is_some() {
                            return Err(anyhow!(crate::Trap::NoAsyncResult));
                        }
                    } else {
                        instance.exit_instance(callee_instance)?;

                        let lift = instance.get_mut(guest_task)?.lift_result.take().unwrap();

                        assert!(instance.get(guest_task)?.result.is_none());

                        // SAFETY: `result_count` represents the number of core Wasm
                        // results returned, per `wasmparser`.
                        let result = (lift.lift)(store, instance, unsafe {
                            mem::transmute::<&[MaybeUninit<ValRaw>], &[ValRaw]>(
                                &storage[..result_count],
                            )
                        })?;

                        unsafe { flags.set_needs_post_return(false) }

                        if let Some(func) = post_return {
                            let arg = match result_count {
                                0 => ValRaw::i32(0),
                                // SAFETY: `result_count` represents the number of
                                // core Wasm results returned, per `wasmparser`.
                                1 => unsafe { storage[0].assume_init() },
                                _ => unreachable!(),
                            };

                            let mut store = token.as_context_mut(store);

                            // SAFETY: `func` is a valid `*mut VMFuncRef` from
                            // either `wasmtime-cranelift`-generated fused adapter
                            // code or `component::Options`.  Per `wasmparser`
                            // post-return signature validation, we know it takes a
                            // single parameter.
                            store.with_attached_instance(instance, |mut store, _| unsafe {
                                crate::Func::call_unchecked_raw(
                                    &mut store,
                                    func.as_non_null(),
                                    NonNull::new(ptr::slice_from_raw_parts(&arg, 1).cast_mut())
                                        .unwrap(),
                                )
                            })?;
                        }

                        unsafe { flags.set_may_enter(true) }

                        instance.task_complete(store, guest_task, result, Status::Returned)?;
                    }

                    instance.maybe_pop_call_context(store.store_opaque_mut(), guest_task)?;

                    Ok(())
                },
            )
        };

        self.push_high_priority(WorkItem::GuestCall(GuestCall {
            task: guest_task,
            kind: GuestCallKind::Start(fun),
        }));

        Ok(())
    }

    /// Prepare (but do not start) a guest->guest call.
    ///
    /// SAFETY: All the pointer arguments must be valid pointers to guest
    /// entities.  In addition the caller must confer exclusive access to the
    /// store to which the passed `&mut ComponentInstance` belongs, which must
    /// have a data type parameter of `T`.
    unsafe fn prepare_call<T: 'static>(
        &mut self,
        store: StoreContextMut<T>,
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
        let token = StoreToken::new(store);
        let new_task = GuestTask::new(
            self,
            Box::new(move |store, instance, dst| {
                // SAFETY: This `ComponentInstance` belongs to the store in
                // which it resides, so if it is valid then so is its store.
                // Furthermore, this closure is only called (transitively) from
                // `ComponentInstance::poll_until`, which has exclusive access
                // to both the `ComponentInstance` and the store.
                //
                // In addition, the store's data type is known to be `T` because
                // this closure will have been called with the same
                // `ComponentInstance` that was passed to the outer
                // `prepare_call` scope.  See
                // `ComponentInstance::{poll_until,handle_work_item,handle_guest_call}`,
                // where we pop the work item containing this closure and pass
                // it the same `ComponentInstance`.
                let mut store = token.as_context_mut(store);
                assert!(dst.len() <= MAX_FLAT_PARAMS);
                let mut src = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
                let count = match caller_info {
                    CallerInfo::Async { params, .. } => {
                        src[0] = MaybeUninit::new(ValRaw::u32(params));
                        1
                    }
                    CallerInfo::Sync { params, .. } => {
                        // SAFETY: Transmuting from `&[T]` to
                        // `&[MaybeUninit<T>]` should be sound for any `T`.
                        src[..params.len()].copy_from_slice(unsafe {
                            mem::transmute::<&[ValRaw], &[MaybeUninit<ValRaw>]>(&params)
                        });
                        params.len()
                    }
                };
                // SAFETY: `start` is a valid `*mut VMFuncRef` from
                // `wasmtime-cranelift`-generated fused adapter code.  Based on
                // how it was constructed (see
                // `wasmtime_environ::fact::trampoline::Compiler::compile_async_start_adapter`
                // for details) we know it takes count parameters and returns
                // `dst.len()` results.
                store.with_attached_instance(instance, |mut store, _| unsafe {
                    crate::Func::call_unchecked_raw(
                        &mut store,
                        start.as_non_null(),
                        NonNull::new(
                            &mut src[..count.max(dst.len())] as *mut [MaybeUninit<ValRaw>] as _,
                        )
                        .unwrap(),
                    )
                })?;
                dst.copy_from_slice(&src[..dst.len()]);
                let task = instance.guest_task().unwrap();
                Waitable::Guest(task).set_event(
                    instance,
                    Some(Event::Subtask {
                        status: Status::Started,
                    }),
                )?;
                Ok(())
            }),
            LiftResult {
                lift: Box::new(move |store, instance, src| {
                    // SAFETY: See comment in closure passed as `lower_params`
                    // parameter above.
                    let mut store = token.as_context_mut(store);
                    let mut my_src = src.to_owned(); // TODO: use stack to avoid allocation?
                    if let ResultInfo::Heap { results } = &result_info {
                        my_src.push(ValRaw::u32(*results));
                    }
                    // SAFETY: `return_` is a valid `*mut VMFuncRef` from
                    // `wasmtime-cranelift`-generated fused adapter code.  Based
                    // on how it was constructed (see
                    // `wasmtime_environ::fact::trampoline::Compiler::compile_async_return_adapter`
                    // for details) we know it takes `src.len()` parameters and
                    // returns up to 1 result.
                    store.with_attached_instance(instance, |mut store, _| unsafe {
                        crate::Func::call_unchecked_raw(
                            &mut store,
                            return_.as_non_null(),
                            my_src.as_mut_slice().into(),
                        )
                    })?;
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

        if let Some(old_task) = old_task {
            if !self.may_enter(guest_task) {
                bail!(crate::Trap::CannotEnterComponent);
            }

            self.get_mut(old_task)?.subtasks.insert(guest_task);
        };

        *self.guest_task() = Some(guest_task);
        log::trace!("pushed {guest_task:?} as current task; old task was {old_task:?}");

        Ok(())
    }

    unsafe fn call_callback<T>(
        &mut self,
        mut store: StoreContextMut<T>,
        callee_instance: RuntimeComponentInstanceIndex,
        function: SendSyncPtr<VMFuncRef>,
        event: Event,
        handle: u32,
    ) -> Result<u32> {
        // SAFETY: This `ComponentInstance` belongs to the store in which it
        // resides, so if it is valid then so is its store.  Furthermore, this
        // function is only called (transitively) from
        // `ComponentInstance::poll_until`, which has exclusive access to both
        // the `ComponentInstance` and the store.
        //
        // In addition, the store's data type is known to be `T` because this
        // function will have been called with the same `ComponentInstance` that
        // was in scope in `ComponentInstance::start_call` or `prepare_call` --
        // the two functions where this function is monomophized and stored as
        // an `fn(..)` in `GuestTask::callback`.  See
        // `ComponentInstance::{poll_until,handle_work_item,handle_guest_call}`,
        // where we pop the work item containing the pointer to this function
        // and pass it the same `ComponentInstance`.
        let mut flags = self.instance_flags(callee_instance);

        let (ordinal, result) = event.parts();
        let params = &mut [
            ValRaw::u32(ordinal),
            ValRaw::u32(handle),
            ValRaw::u32(result),
        ];
        // SAFETY: `func` is a valid `*mut VMFuncRef` from either
        // `wasmtime-cranelift`-generated fused adapter code or
        // `component::Options`.  Per `wasmparser` callback signature
        // validation, we know it takes three parameters and returns one.
        store.with_attached_instance(self, |mut store, _| unsafe {
            flags.set_may_enter(false);
            crate::Func::call_unchecked_raw(
                &mut store,
                function.as_non_null(),
                params.as_mut_slice().into(),
            )?;
            flags.set_may_enter(true);
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(params[0].get_u32())
    }

    /// Start a guest->guest call previously prepared using
    /// `Self::prepare_call`.
    ///
    /// SAFETY: All the pointer arguments must be valid pointers to guest
    /// entities.
    unsafe fn start_call<T: 'static>(
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
            let token = StoreToken::new(store.as_context_mut());
            let callback = SendSyncPtr::new(NonNull::new(callback).unwrap());
            task.callback = Some(Box::new(
                move |store, instance, runtime_instance, event, handle| {
                    let store = token.as_context_mut(store);
                    unsafe {
                        instance.call_callback::<T>(
                            store,
                            runtime_instance,
                            callback,
                            event,
                            handle,
                        )
                    }
                },
            ));
        }

        let Caller::Guest {
            task: caller,
            instance,
        } = &task.caller
        else {
            // As of this writing, `start_call` is only used for
            // guest->guest calls.
            unreachable!()
        };
        let caller = *caller;
        let caller_instance = *instance;

        let callee_instance = task.instance;

        self.queue_call(
            store.as_context_mut(),
            guest_task,
            callee,
            param_count,
            result_count,
            if callback.is_null() {
                None
            } else {
                Some(self.instance_flags(callee_instance))
            },
            (flags & EXIT_FLAG_ASYNC_CALLEE) != 0,
            NonNull::new(callback).map(SendSyncPtr::new),
            NonNull::new(post_return).map(SendSyncPtr::new),
        )?;

        let set = self.get_mut(caller)?.sync_call_set;
        Waitable::Guest(guest_task).join(self, Some(set))?;

        let (status, waitable) = loop {
            self.suspend(
                store.0.traitobj_mut(),
                SuspendReason::Waiting { set, task: caller },
            )?;

            let event = Waitable::Guest(guest_task).take_event(self)?;
            let Some(Event::Subtask { status }) = event else {
                unreachable!();
            };

            log::trace!("status {status:?} for {guest_task:?}");

            if status == Status::Returned {
                break (status, None);
            } else if async_caller {
                break (
                    status,
                    Some(
                        self.waitable_tables()[caller_instance]
                            .insert(guest_task.rep(), WaitableState::GuestTask)?,
                    ),
                );
            };
        };

        Waitable::Guest(guest_task).join(self, None)?;

        if let Some(storage) = storage {
            if let Some(result) = self.get_mut(guest_task)?.sync_result.take() {
                if let Some(result) = result {
                    storage[0] = MaybeUninit::new(result);
                }
            } else {
                return Err(anyhow!(crate::Trap::NoAsyncResult));
            }
        }

        *self.guest_task() = Some(caller);
        log::trace!("popped current task {guest_task:?}; new task is {caller:?}");

        Ok(status.pack(waitable))
    }

    pub(crate) fn wrap_call<T: 'static, F, P, R>(
        &mut self,
        store: StoreContextMut<T>,
        closure: Arc<F>,
        params: P,
    ) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'static>>
    where
        T: 'static,
        F: for<'a> Fn(
                &'a mut Accessor<T>,
                P,
            ) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'a>>
            + Send
            + Sync
            + 'static,
        P: Send + Sync + 'static,
        R: Send + Sync + 'static,
    {
        // SAFETY: The `get_store` function we pass here is backed by a
        // thread-local variable which `poll_with_state` will populate and reset
        // with valid pointers to the store data and the store itself each time
        // the returned future is polled, respectively.
        let mut accessor = unsafe { Accessor::new(get_store, |x| x, spawn_task, self.instance()) };
        let mut future = Box::pin(async move { closure(&mut accessor, params).await });
        // SAFETY: `poll_with_state` will populate and reset the thread-local
        // state as described above.
        //
        // This is sound because `ComponentInstance::poll_until` is the only
        // place we will poll this future (see the doc comment on this
        // function), and it has exclusive access to the store and
        // `ComponentInstance` when doing so.
        let token = StoreToken::new(store);
        Box::pin(future::poll_fn(move |cx| {
            poll_with_state(token, cx, future.as_mut())
        }))
    }

    /// Poll the specified future once, and if it returns `Pending`, add it to
    /// the set of futures to be polled as part of this instance's event loop
    /// until it completes.
    pub(crate) fn first_poll<T: 'static, R: Send + Sync + 'static>(
        &mut self,
        mut store: StoreContextMut<T>,
        future: impl Future<Output = Result<R>> + Send + 'static,
        caller_instance: RuntimeComponentInstanceIndex,
        lower: impl FnOnce(StoreContextMut<T>, &mut ComponentInstance, R) -> Result<()> + Send + 'static,
    ) -> Result<Option<u32>> {
        let caller = self.guest_task().unwrap();
        let wrapped = Arc::new(Mutex::new(AbortWrapper::Unpolled(Box::pin(future))));
        let task = self.push(HostTask::new(
            caller_instance,
            Some(AbortHandle::new(wrapped.clone())),
        ))?;
        let token = StoreToken::new(store.as_context_mut());

        let future = future::poll_fn({
            let mut call_context = None;
            move |cx| {
                let mut wrapped = wrapped.try_lock().unwrap();

                let inner = mem::replace(DerefMut::deref_mut(&mut wrapped), AbortWrapper::Aborted);
                if let AbortWrapper::Unpolled(mut future)
                | AbortWrapper::Polled { mut future, .. } = inner
                {
                    with_local_instance(|store, _| {
                        if let Some(call_context) = call_context.take() {
                            log::trace!("push call context for {task:?}");
                            token
                                .as_context_mut(store)
                                .0
                                .component_resource_state()
                                .0
                                .push(call_context);
                        }
                    });

                    let result = future.as_mut().poll(cx);

                    *DerefMut::deref_mut(&mut wrapped) = AbortWrapper::Polled {
                        future,
                        waker: cx.waker().clone(),
                    };

                    match result {
                        Poll::Ready(output) => Poll::Ready(Some(output)),
                        Poll::Pending => {
                            with_local_instance(|store, _| {
                                log::trace!("pop call context for {task:?}");
                                call_context = Some(
                                    token
                                        .as_context_mut(store)
                                        .0
                                        .component_resource_state()
                                        .0
                                        .pop()
                                        .unwrap(),
                                );
                            });
                            Poll::Pending
                        }
                    }
                } else {
                    Poll::Ready(None)
                }
            }
        });

        log::trace!("new host task child of {caller:?}: {task:?}");
        let token = StoreToken::new(store.as_context_mut());
        let mut future = Box::pin(future.map(move |result| {
            if let Some(result) = result {
                HostTaskOutput::Function(Box::new(move |store, instance| {
                    let store = token.as_context_mut(store);
                    lower(store, instance, result?)?;
                    instance.get_mut(task)?.abort_handle.take();
                    Waitable::Host(task).set_event(
                        instance,
                        Some(Event::Subtask {
                            status: Status::Returned,
                        }),
                    )?;

                    Ok(())
                }))
            } else {
                // Task was cancelled; nothing left to do.
                HostTaskOutput::Result(Ok(()))
            }
        })) as HostTaskFuture;

        let poll = unsafe {
            poll_with_local_instance(
                VMStoreRawPtr(store.traitobj()),
                SendSyncPtr::new(NonNull::new(self).unwrap()),
                &mut future.as_mut(),
                &mut Context::from_waker(&dummy_waker()),
            )
        };

        Ok(match poll {
            Poll::Ready(output) => {
                output.consume(store.0.traitobj_mut(), self)?;
                log::trace!("delete host task {task:?} (already ready)");
                self.delete(task)?;
                None
            }
            Poll::Pending => {
                self.push_future(future);
                let handle = self.waitable_tables()[caller_instance]
                    .insert(task.rep(), WaitableState::HostTask)?;
                log::trace!(
                    "assign {task:?} handle {handle} for {caller:?} instance {caller_instance:?}"
                );
                Some(handle)
            }
        })
    }

    pub(crate) fn poll_and_block<R: Send + Sync + 'static>(
        &mut self,
        store: &mut dyn VMStore,
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
            .with_context(|| format!("bad handle: {caller:?}"))?
            .result
            .take();
        let task = self.push(HostTask::new(caller_instance, None))?;

        log::trace!("new host task child of {caller:?}: {task:?}");
        let mut future = Box::pin(future.map(move |result| {
            HostTaskOutput::Function(Box::new(move |_, instance| {
                instance.get_mut(caller)?.result = Some(Box::new(result?) as _);

                Waitable::Host(task).set_event(
                    instance,
                    Some(Event::Subtask {
                        status: Status::Returned,
                    }),
                )?;

                Ok(())
            }))
        })) as HostTaskFuture;

        let poll = unsafe {
            poll_with_local_instance(
                VMStoreRawPtr(store.traitobj()),
                SendSyncPtr::new(NonNull::new(self).unwrap()),
                &mut future.as_mut(),
                &mut Context::from_waker(&dummy_waker()),
            )
        };

        Ok(match poll {
            Poll::Ready(output) => {
                output.consume(store, self)?;
                log::trace!("delete host task {task:?} (already ready)");
                self.delete(task)?;
                let result = *mem::replace(&mut self.get_mut(caller)?.result, old_result)
                    .unwrap()
                    .downcast()
                    .unwrap();
                result
            }
            Poll::Pending => {
                self.push_future(future);

                let set = self.get_mut(caller)?.sync_call_set;
                Waitable::Host(task).join(self, Some(set))?;

                self.suspend(store, SuspendReason::Waiting { set, task: caller })?;

                let result = self.get_mut(caller)?.result.take().unwrap();
                self.get_mut(caller)?.result = old_result;
                *result.downcast().unwrap()
            }
        })
    }

    async fn poll_until<T, R: Send + Sync + 'static>(
        &mut self,
        store: StoreContextMut<'_, T>,
        future: impl Future<Output = R> + Send,
    ) -> Result<R> {
        // Here we smuggle the `ComponentInstance` pointer into the future so we
        // can use it while polling without upsetting the borrow checker given
        // that we're also mutably borrowing `ConcurrentState::futures` to poll
        // it.
        //
        // SAFETY: This is morally equivalent to a split borrow, since we are
        // careful not to touch `ConcurrentState::futures` at any time while
        // polling.  See `ComponentInstance::push_future` which, explicitly
        // defers touching `futures` by queuing a work item which we'll run only
        // _after_ polling.
        let instance = SendSyncPtr::new(NonNull::new(self).unwrap());
        let store_ptr = VMStoreRawPtr(store.traitobj());
        let mut future = pin!(future);

        unsafe fn consume(
            store: VMStoreRawPtr,
            instance: SendSyncPtr<ComponentInstance>,
            output: HostTaskOutput,
        ) -> Result<()> {
            let (store, instance) = unsafe { (&mut *store.0.as_ptr(), &mut *instance.as_ptr()) };
            output.consume(store, instance)
        }

        loop {
            let mut futures = self
                .concurrent_state
                .futures
                .get_mut()
                .unwrap()
                .take()
                .unwrap();
            let mut next = pin!(futures.next());

            let result = future::poll_fn(|cx| {
                if let Poll::Ready(value) =
                    unsafe { poll_with_local_instance(store_ptr, instance, &mut future, cx) }
                {
                    return Poll::Ready(Ok(Either::Left(value)));
                }

                let next =
                    match unsafe { poll_with_local_instance(store_ptr, instance, &mut next, cx) } {
                        Poll::Ready(Some(output)) => {
                            // SAFETY: See SAFETY comment in outer scope above.
                            if let Err(e) = unsafe { consume(store_ptr, instance, output) } {
                                return Poll::Ready(Err(e));
                            }
                            Poll::Ready(true)
                        }
                        Poll::Ready(None) => Poll::Ready(false),
                        Poll::Pending => Poll::Pending,
                    };

                // SAFETY: See SAFETY comment in outer scope above.
                let me = unsafe { &mut *instance.as_ptr() };
                let ready = mem::take(&mut me.concurrent_state.high_priority);
                let ready = if ready.is_empty() {
                    let ready = mem::take(&mut me.concurrent_state.low_priority);
                    if ready.is_empty() {
                        return match next {
                            Poll::Ready(true) => Poll::Ready(Ok(Either::Right(Vec::new()))),
                            // Here we return an error indicating we can't make
                            // further progress.  The underlying assumption is
                            // that `future` depends on this component instance
                            // making such progress, and thus there's no point
                            // in continuing to poll it given we've run out of
                            // work to do.
                            //
                            // Note that we'd also reach this point if the host
                            // embedder passed e.g. a `std::future::Pending` to
                            // `Instance::run`, in which case we'd return a
                            // "deadlock" error even when any and all tasks have
                            // completed normally.  However, that's not how
                            // `Instance::run` is intended (and documented) to
                            // be used, so it seems reasonable to lump that case
                            // in with "real" deadlocks.
                            //
                            // TODO: Once we've added host APIs for cancelling
                            // in-progress tasks, we can return some other,
                            // non-error value here, treating it as "normal" and
                            // giving the host embedder a chance to intervene by
                            // cancelling one or more tasks and/or starting new
                            // tasks capable of waking the existing ones.
                            Poll::Ready(false) => {
                                Poll::Ready(Err(anyhow!(crate::Trap::AsyncDeadlock)))
                            }
                            Poll::Pending => Poll::Pending,
                        };
                    } else {
                        ready
                    }
                } else {
                    ready
                };

                Poll::Ready(Ok(Either::Right(ready)))
            })
            .await;

            *self.concurrent_state.futures.get_mut().unwrap() = Some(futures);

            match result? {
                Either::Left(value) => break Ok(value),
                Either::Right(ready) => {
                    for item in ready {
                        self.handle_work_item(VMStoreRawPtr(store.0.traitobj()), item)
                            .await?;
                    }
                }
            }
        }
    }

    fn get_event(
        &mut self,
        guest_task: TableId<GuestTask>,
        instance: RuntimeComponentInstanceIndex,
        set: Option<TableId<WaitableSet>>,
    ) -> Result<Option<(Event, Option<(Waitable, u32)>)>> {
        Ok(
            if let Some(event) = self.get_mut(guest_task)?.event.take() {
                log::trace!("deliver event {event:?} to {guest_task:?}");

                Some((event, None))
            } else if let Some((set, waitable)) = set
                .and_then(|set| {
                    self.get_mut(set)
                        .map(|v| v.ready.pop_first().map(|v| (set, v)))
                        .transpose()
                })
                .transpose()?
            {
                let event = waitable.common(self)?.event.take().unwrap();

                log::trace!(
                    "deliver event {event:?} to {guest_task:?} for {waitable:?}; set {set:?}"
                );

                let entry = self.waitable_tables()[instance].get_mut_by_rep(waitable.rep());
                let Some((
                    handle,
                    WaitableState::HostTask
                    | WaitableState::GuestTask
                    | WaitableState::Stream(..)
                    | WaitableState::Future(..),
                )) = entry
                else {
                    bail!("handle not found for waitable rep {waitable:?} instance {instance:?}");
                };

                waitable.on_delivery(self, event);

                Some((event, Some((waitable, handle))))
            } else {
                None
            },
        )
    }

    fn handle_guest_call(&mut self, store: &mut dyn VMStore, call: GuestCall) -> Result<()> {
        match call.kind {
            GuestCallKind::DeliverEvent { instance, set } => {
                let (event, waitable) = self.get_event(call.task, instance, set)?.unwrap();
                let task = self.get_mut(call.task)?;
                let instance = task.instance;
                let handle = waitable.map(|(_, v)| v).unwrap_or(0);

                log::trace!(
                    "use callback to deliver event {event:?} to {:?} for {waitable:?}",
                    call.task,
                );

                let old_task = self.guest_task().replace(call.task);
                log::trace!(
                    "GuestCallKind::DeliverEvent: replaced {old_task:?} with {:?} as current task",
                    call.task
                );

                self.maybe_push_call_context(store.store_opaque_mut(), call.task)?;

                self.enter_instance(instance);

                let callback = self.get_mut(call.task)?.callback.take().unwrap();

                let code = callback(store, self, instance, event, handle)?;

                self.get_mut(call.task)?.callback = Some(callback);

                self.exit_instance(instance)?;

                self.maybe_pop_call_context(store.store_opaque_mut(), call.task)?;

                self.handle_callback_code(call.task, instance, code, Event::None)?;

                *self.guest_task() = old_task;
                log::trace!("GuestCallKind::DeliverEvent: restored {old_task:?} as current task");
            }
            GuestCallKind::Start(fun) => {
                fun(store, self)?;
            }
        }

        Ok(())
    }

    async fn resume_fiber(
        &mut self,
        store: VMStoreRawPtr,
        fiber: StoreFiber<'static>,
    ) -> Result<()> {
        let old_task = *self.guest_task();
        log::trace!("resume_fiber: save current task {old_task:?}");
        let guard_range = fiber.guard_range();
        let mut fiber = Some(fiber);
        // Here we pass control of the store to the fiber, which requires
        // smuggling it as a `VMStoreRawPtr` in order to ensure the future is
        // `Send`.
        //
        // By the time the future returned by `poll_fn` completes, we'll have
        // exclusive access to it again.
        //
        // SAFETY: This `ComponentInstance` belongs to the store in which it
        // resides, so if it is valid then so is its store.  Furthermore, this
        // function is only called (transitively) from
        // `ComponentInstance::poll_until`, which has exclusive access to both
        // the `ComponentInstance` and the store.
        let fiber = unsafe {
            poll_fn(store, guard_range, move |_, mut store| {
                match resume_fiber(fiber.as_mut().unwrap(), store.take(), Ok(())) {
                    Ok(Ok((_, result))) => Ok(result.map(|()| None)),
                    Ok(Err(store)) => {
                        if store.is_some() {
                            Ok(Ok(fiber.take()))
                        } else {
                            Err(None)
                        }
                    }
                    Err(error) => Ok(Err(error)),
                }
            })
        }
        .await?;

        *self.guest_task() = old_task;
        log::trace!("resume_fiber: restore current task {old_task:?}");

        if let Some(fiber) = fiber {
            match self.suspend_reason().take().unwrap() {
                SuspendReason::NeedWork => {
                    if self.worker().is_none() {
                        *self.worker() = Some(fiber);
                    }
                }
                SuspendReason::Yielding { .. } => {
                    self.push_low_priority(WorkItem::ResumeFiber(fiber));
                }
                SuspendReason::Waiting { set, task } => {
                    let old = self
                        .get_mut(set)?
                        .waiting
                        .insert(task, WaitMode::Fiber(fiber));
                    assert!(old.is_none());
                }
            }
        }

        Ok(())
    }

    async fn handle_work_item(&mut self, store: VMStoreRawPtr, item: WorkItem) -> Result<()> {
        log::trace!("handle work item {item:?}");
        match item {
            WorkItem::PushFuture(future) => {
                self.concurrent_state
                    .futures
                    .get_mut()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .push(future.into_inner().unwrap());
            }
            WorkItem::ResumeFiber(fiber) => {
                self.resume_fiber(store, fiber).await?;
            }
            WorkItem::GuestCall(call) => {
                if call.is_ready(self)? {
                    self.run_on_worker(store, call).await?;
                } else {
                    let task = self.get_mut(call.task)?;
                    if !task.starting_sent {
                        task.starting_sent = true;
                        if let GuestCallKind::Start(_) = &call.kind {
                            Waitable::Guest(call.task).set_event(
                                self,
                                Some(Event::Subtask {
                                    status: Status::Starting,
                                }),
                            )?;
                        }
                    }

                    let instance = self.get(call.task)?.instance;
                    self.instance_state(instance)
                        .pending
                        .insert(call.task, call.kind);
                }
            }
            WorkItem::Poll(params) => {
                if self.get_mut(params.task)?.event.is_some()
                    || !self.get_mut(params.set)?.ready.is_empty()
                {
                    self.push_high_priority(WorkItem::GuestCall(GuestCall {
                        task: params.task,
                        kind: GuestCallKind::DeliverEvent {
                            instance: params.instance,
                            set: Some(params.set),
                        },
                    }));
                } else {
                    self.get_mut(params.task)?.event = Some(Event::None);
                    self.push_high_priority(WorkItem::GuestCall(GuestCall {
                        task: params.task,
                        kind: GuestCallKind::DeliverEvent {
                            instance: params.instance,
                            set: Some(params.set),
                        },
                    }));
                }
            }
        }

        Ok(())
    }

    async fn run_on_worker(&mut self, store: VMStoreRawPtr, call: GuestCall) -> Result<()> {
        let worker = if let Some(fiber) = self.worker().take() {
            fiber
        } else {
            // Here we smuggle the `ComponentInstance` pointer into the closure
            // so that the fiber can use it without upsetting the borrow
            // checker.
            //
            // SAFETY: We will only resume this fiber in either
            // `ComponentInstance::handle_work_item` or
            // `ComponentInstnace::run_on_worker`, where we'll have exclusive
            // access to the same `ComponentInstance` and thus be able to grant
            // the same access to the fiber we're resuming.
            //
            // TODO: Consider adding `*mut ComponentInstance` parameters to
            // `StoreFiber`'s `suspend` and `resume` signatures to make this
            // handoff more explicit.
            let instance = self as *mut Self;
            unsafe {
                make_fiber(store, move |store| {
                    let instance = &mut *instance;
                    loop {
                        let call = instance.guest_call().take().unwrap();
                        instance.handle_guest_call(&mut *store, call)?;

                        instance.suspend(&mut *store, SuspendReason::NeedWork)?;
                    }
                })?
            }
        };

        assert!(self.guest_call().is_none());
        *self.guest_call() = Some(call);

        self.resume_fiber(store, worker).await
    }

    fn suspend(&mut self, store: &mut dyn VMStore, reason: SuspendReason) -> Result<()> {
        log::trace!("suspend fiber: {reason:?}");

        let task = match &reason {
            SuspendReason::Yielding { task } | SuspendReason::Waiting { task, .. } => Some(*task),
            SuspendReason::NeedWork => None,
        };

        let old_guest_task = if let Some(task) = task {
            self.maybe_pop_call_context(store.store_opaque_mut(), task)?;
            *self.guest_task()
        } else {
            None
        };

        assert!(self.suspend_reason().is_none());
        *self.suspend_reason() = Some(reason);

        let async_cx = AsyncCx::new(store.store_opaque_mut());
        // SAFETY: This is only ever called from a fiber that belongs to this
        // store (and would in any case panic if called from outside any fiber).
        unsafe {
            async_cx.suspend(Some(store))?;
        }

        if let Some(task) = task {
            *self.guest_task() = old_guest_task;
            self.maybe_push_call_context(store.store_opaque_mut(), task)?;
        }

        Ok(())
    }

    pub(crate) fn task_return(
        &mut self,
        store: &mut dyn VMStore,
        ty: TypeTupleIndex,
        memory: *mut VMMemoryDefinition,
        string_encoding: u8,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()> {
        // SAFETY: The `wasmtime_cranelift`-generated code that calls this
        // method will have ensured that `storage` is a valid pointer containing
        // at least `storage_len` items.
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

        log::trace!("task.return for {guest_task:?}");

        let result = (lift.lift)(store, self, storage)?;

        self.task_complete(store, guest_task, result, Status::Returned)
    }

    pub(crate) fn task_cancel(
        &mut self,
        store: &mut dyn VMStore,
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

        log::trace!("task.cancel for {guest_task:?}");

        self.task_complete(
            store,
            guest_task,
            Box::new(DummyResult),
            Status::ReturnCancelled,
        )
    }

    fn task_complete(
        &mut self,
        store: &mut dyn VMStore,
        guest_task: TableId<GuestTask>,
        result: Box<dyn Any + Send + Sync>,
        status: Status,
    ) -> Result<()> {
        let (calls, host_table, _) = store.store_opaque_mut().component_resource_state();
        ResourceTables {
            calls,
            host_table: Some(host_table),
            guest: Some(self.guest_tables()),
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
            Waitable::Guest(guest_task).set_event(self, Some(Event::Subtask { status }))?;
        }

        Ok(())
    }

    pub(crate) fn backpressure_set(
        &mut self,
        caller_instance: RuntimeComponentInstanceIndex,
        enabled: u32,
    ) -> Result<()> {
        let state = self.instance_state(caller_instance);
        let old = state.backpressure;
        let new = enabled != 0;
        state.backpressure = new;

        if old && !new {
            self.partition_pending(caller_instance)?;
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
        log::trace!("new waitable set {set:?} (handle {handle})");
        Ok(handle)
    }

    pub(crate) fn waitable_set_wait(
        &mut self,
        store: &mut dyn VMStore,
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
            store,
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
        store: &mut dyn VMStore,
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
            store,
            async_,
            WaitableCheck::Poll(WaitableCheckParams {
                set: TableId::new(rep),
                memory,
                payload,
                caller_instance,
            }),
        )
    }

    pub(crate) fn yield_(&mut self, store: &mut dyn VMStore, async_: bool) -> Result<bool> {
        self.waitable_check(store, async_, WaitableCheck::Yield)
            .map(|_code| {
                // TODO: plumb cancellation to here
                false
            })
    }

    pub(crate) fn waitable_check(
        &mut self,
        store: &mut dyn VMStore,
        async_: bool,
        check: WaitableCheck,
    ) -> Result<u32> {
        if async_ {
            bail!(
                "todo: async `waitable-set.wait`, `waitable-set.poll`, and `yield` not yet implemented"
            );
        }

        let guest_task = self.guest_task().unwrap();

        let (wait, set) = match &check {
            WaitableCheck::Wait(params) => (true, Some(params.set)),
            WaitableCheck::Poll(params) => (false, Some(params.set)),
            WaitableCheck::Yield => (false, None),
        };

        self.suspend(store, SuspendReason::Yielding { task: guest_task })?;

        log::trace!("waitable check for {guest_task:?}; set {set:?}");

        let task = self.get(guest_task)?;

        if wait && task.callback.is_some() {
            bail!("cannot call `task.wait` from async-lifted export with callback");
        }

        if wait {
            let set = set.unwrap();

            if task.event.is_none() && self.get(set)?.ready.is_empty() {
                let old = self.get_mut(guest_task)?.waiting_on.replace(set);
                assert!(old.is_none());

                self.suspend(
                    store,
                    SuspendReason::Waiting {
                        set,
                        task: guest_task,
                    },
                )?;
            }
        }

        log::trace!("waitable check for {guest_task:?}; set {set:?}, part two");

        let result = match check {
            WaitableCheck::Wait(params) | WaitableCheck::Poll(params) => {
                let event = self.get_event(guest_task, params.caller_instance, Some(params.set))?;

                let (ordinal, handle, result) = if wait {
                    let (event, waitable) = event.unwrap();
                    let handle = waitable.map(|(_, v)| v).unwrap_or(0);
                    let (ordinal, result) = event.parts();
                    (ordinal, handle, result)
                } else {
                    if let Some((event, waitable)) = event {
                        let handle = waitable.map(|(_, v)| v).unwrap_or(0);
                        let (ordinal, result) = event.parts();
                        (ordinal, handle, result)
                    } else {
                        log::trace!(
                            "no events ready to deliver via waitable-set.poll to {guest_task:?}; set {:?}",
                            params.set
                        );
                        let (ordinal, result) = Event::None.parts();
                        (ordinal, 0, result)
                    }
                };
                let store = store.store_opaque_mut();
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
            "waitable {waitable:?} (handle {waitable_handle}) join set {set:?} (handle {set_handle})",
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
        let (waitable, expected_caller_instance) = match state {
            WaitableState::HostTask => {
                let id = TableId::<HostTask>::new(rep);
                (Waitable::Host(id), self.get(id)?.caller_instance)
            }
            WaitableState::GuestTask => {
                let id = TableId::<GuestTask>::new(rep);
                if let Caller::Guest { instance, .. } = &self.get(id)?.caller {
                    (Waitable::Guest(id), *instance)
                } else {
                    unreachable!()
                }
            }
            _ => bail!("invalid task handle: {task_id}"),
        };
        assert_eq!(expected_caller_instance, caller_instance);
        log::trace!("subtask_drop {waitable:?} (handle {task_id})");
        Ok(())
    }

    pub(crate) fn subtask_cancel(
        &mut self,
        store: &mut dyn VMStore,
        caller_instance: RuntimeComponentInstanceIndex,
        async_: bool,
        task_id: u32,
    ) -> Result<u32> {
        let (rep, state) = self.waitable_tables()[caller_instance].get_mut_by_index(task_id)?;
        let (waitable, expected_caller_instance) = match state {
            WaitableState::HostTask => {
                let id = TableId::<HostTask>::new(rep);
                (Waitable::Host(id), self.get(id)?.caller_instance)
            }
            WaitableState::GuestTask => {
                let id = TableId::<GuestTask>::new(rep);
                if let Caller::Guest { instance, .. } = &self.get(id)?.caller {
                    (Waitable::Guest(id), *instance)
                } else {
                    unreachable!()
                }
            }
            _ => bail!("invalid task handle: {task_id}"),
        };
        assert_eq!(expected_caller_instance, caller_instance);

        log::trace!("subtask_cancel {waitable:?} (handle {task_id})");

        if let Waitable::Host(host_task) = waitable {
            if let Some(handle) = self.get_mut(host_task)?.abort_handle.take() {
                handle.abort();
                return Ok(Status::ReturnCancelled as u32);
            }
        } else {
            let caller = self.guest_task().unwrap();
            let guest_task = TableId::<GuestTask>::new(rep);
            let task = self.get_mut(guest_task)?;
            if task.lower_params.is_some() {
                // Not yet started; cancel and remove from pending
                let callee_instance = task.instance;

                let kind = self
                    .instance_state(callee_instance)
                    .pending
                    .remove(&guest_task);

                if kind.is_none() {
                    bail!("`subtask.cancel` called after terminal status delivered");
                }

                return Ok(Status::StartCancelled as u32);
            } else if task.lift_result.is_some() {
                // Started, but not yet returned or cancelled; send the
                // `CANCELLED` event
                task.cancel_sent = true;
                // Note that this might overwrite an event that was set earlier
                // (e.g. `Event::None` if the task is yielding, or
                // `Event::Cancelled` if it was already cancelled), but that's
                // okay -- this should supersede the previous state.
                task.event = Some(Event::Cancelled);
                if let Some(set) = task.waiting_on.take() {
                    let item = match self.get_mut(set)?.waiting.remove(&guest_task).unwrap() {
                        WaitMode::Fiber(fiber) => WorkItem::ResumeFiber(fiber),
                        WaitMode::Callback(instance) => WorkItem::GuestCall(GuestCall {
                            task: guest_task,
                            kind: GuestCallKind::DeliverEvent {
                                instance,
                                set: None,
                            },
                        }),
                    };
                    self.push_high_priority(item);

                    self.suspend(store, SuspendReason::Yielding { task: caller })?;
                }

                let task = self.get_mut(guest_task)?;
                if task.lift_result.is_some() {
                    // Still not yet returned or cancelled; if `async_`, return
                    // `BLOCKED`; otherwise wait
                    if async_ {
                        return Ok(BLOCKED);
                    } else {
                        let set = self.get_mut(caller)?.sync_call_set;
                        Waitable::Guest(guest_task).join(self, Some(set))?;

                        self.suspend(store, SuspendReason::Waiting { set, task: caller })?;
                    }
                }
            }
        }

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
        let val = self.get(task)?.context[usize::try_from(slot).unwrap()];
        log::trace!("context_get {task:?} slot {slot} val {val:#x}");
        Ok(val)
    }

    pub(crate) fn context_set(&mut self, slot: u32, val: u32) -> Result<()> {
        let task = self.guest_task().unwrap();
        log::trace!("context_set {task:?} slot {slot} val {val:#x}");
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
    /// must use either this function, `Instance::run_with`, or
    /// `Instance::spawn` to ensure they are polled as part of the correct event
    /// loop.
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
    ///
    /// Note that this function will return a "deadlock" error in either of the
    /// following scenarios:
    ///
    /// - One or more guest tasks are still pending (i.e. have not yet returned,
    /// or, in the case of async-lifted exports with callbacks, have not yet
    /// returned `CALLBACK_CODE_EXIT`) even though all host tasks have completed
    /// all host-owned stream and future handles have been closed, etc.
    ///
    /// - Any and all guest tasks complete normally, but the future passed to
    /// this function continues to return `Pending` when polled.  In that case,
    /// the future presumably does not depend on any guest task making further
    /// progress (since no futher progress can be made) and thus is not an
    /// appropriate future to poll using this function.
    pub async fn run<U: Send, V: Send + Sync + 'static>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fut: impl Future<Output = V> + Send,
    ) -> Result<V> {
        check_recursive_run();
        store
            .as_context_mut()
            .with_detached_instance_async(self, async |store, instance| {
                instance.poll_until(store, fut).await
            })
            .await
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
    /// #     component::{ Component, Linker, Resource, ResourceTable, Accessor },
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
    /// instance.run_with(&mut store, move |accessor: &mut Accessor<_>| Box::pin(async move {
    ///    let (another_resource,) = accessor.with(|mut access| {
    ///        let resource = access.get().table.push(MyResource(42))?;
    ///        Ok::<_, Error>(foo.call_concurrent(access, (resource,)))
    ///    })?.await?;
    ///    accessor.with(|mut access| {
    ///        let value = access.get().table.delete(another_resource)?;
    ///        Ok::<_, Error>(bar.call_concurrent(access, (value.0,)))
    ///    })?.await
    /// })).await??;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run_with<U: Send + 'static, V: Send + Sync + 'static, F>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fun: F,
    ) -> Result<V>
    where
        U: 'static,
        F: for<'a> FnOnce(&'a mut Accessor<U>) -> Pin<Box<dyn Future<Output = V> + Send + 'a>>
            + Send
            + 'static,
    {
        check_recursive_run();
        let future = self.run_with_raw(&mut store, fun);
        self.run(store, future).await
    }

    fn run_with_raw<U: Send + 'static, V: Send + Sync + 'static, F>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        fun: F,
    ) -> Pin<Box<dyn Future<Output = V> + Send + 'static>>
    where
        U: 'static,
        F: for<'a> FnOnce(&'a mut Accessor<U>) -> Pin<Box<dyn Future<Output = V> + Send + 'a>>
            + Send
            + 'static,
    {
        store
            .as_context_mut()
            .with_detached_instance(self, |store, instance| {
                // SAFETY: See corresponding comment in `ComponentInstance::wrap_call`.
                let mut accessor =
                    unsafe { Accessor::new(get_store, |x| x, spawn_task, instance.instance()) };
                let mut future = Box::pin(async move { fun(&mut accessor).await });
                let token = StoreToken::new(store);
                Box::pin(future::poll_fn(move |cx| {
                    poll_with_state::<U, _>(token, cx, future.as_mut())
                }))
            })
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
    pub fn spawn<U: Send + 'static>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        task: impl AccessorTask<U, HasSelf<U>, Result<()>>,
    ) -> AbortHandle {
        let mut store = store.as_context_mut();
        let mut future = self.run_with_raw(store.as_context_mut(), move |accessor| {
            Box::pin(future::ready(accessor.spawn(task)))
        });

        let poll = store.with_detached_instance(self, |store, instance| unsafe {
            poll_with_local_instance(
                VMStoreRawPtr(store.traitobj()),
                SendSyncPtr::new(NonNull::new(instance).unwrap()),
                &mut future.as_mut(),
                &mut Context::from_waker(&dummy_waker()),
            )
        });

        match poll {
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
        store
            .as_context_mut()
            .with_detached_instance(self, |_, instance| instance.spawn(task))
    }
}

/// Trait representing component model ABI async intrinsics and fused adapter
/// helper functions.
///
/// SAFETY (callers): Most of the methods in this trait accept raw pointers,
/// which must be valid for at least the duration of the call.  In addition, the
/// `&mut ComponentInstance` parameter must point to a `ComponentInstance` that
/// belongs to `self` (i.e. both should have been derived from the same
/// `VMComponentContext`).
///
/// SAFETY (implementors): Care must be taken to treat the `&mut Self` and `&mut
/// ComponentInstance` parameters as if they were disjoint borrows and not
/// create aliases by retrieving a `*mut ComponentInstance` from `self` and
/// converting it to a `&mut ComponentInstance`.
pub unsafe trait VMComponentAsyncStore {
    /// A helper function for fused adapter modules involving calls where the
    /// caller is sync-lowered but the callee is async-lifted.
    unsafe fn sync_enter(
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
    unsafe fn sync_exit(
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
    unsafe fn async_enter(
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
    unsafe fn async_exit(
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
    unsafe fn future_write(
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
    unsafe fn future_read(
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
    unsafe fn stream_write(
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
    unsafe fn stream_read(
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
    unsafe fn flat_stream_write(
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
    unsafe fn flat_stream_read(
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
    unsafe fn error_context_debug_message(
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

/// SAFETY: See trait docs.
unsafe impl<T: 'static> VMComponentAsyncStore for StoreInner<T> {
    unsafe fn sync_enter(
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
        instance.prepare_call(
            StoreContextMut(self),
            start,
            return_,
            caller_instance,
            callee_instance,
            task_return_type,
            memory,
            string_encoding,
            CallerInfo::Sync {
                // SAFETY: The `wasmtime_cranelift`-generated code that calls
                // this method will have ensured that `storage` is a valid
                // pointer containing at least `storage_len` items.
                params: unsafe { std::slice::from_raw_parts(storage, storage_len) }.to_vec(),
                result_count,
            },
        )
    }

    unsafe fn sync_exit(
        &mut self,
        instance: &mut ComponentInstance,
        callback: *mut VMFuncRef,
        callee: *mut VMFuncRef,
        param_count: u32,
        storage: *mut MaybeUninit<ValRaw>,
        storage_len: usize,
    ) -> Result<()> {
        instance
            .start_call(
                StoreContextMut(self),
                callback,
                ptr::null_mut(),
                callee,
                param_count,
                1,
                EXIT_FLAG_ASYNC_CALLEE,
                // SAFETY: The `wasmtime_cranelift`-generated code that calls
                // this method will have ensured that `storage` is a valid
                // pointer containing at least `storage_len` items.
                Some(unsafe { std::slice::from_raw_parts_mut(storage, storage_len) }),
            )
            .map(drop)
    }

    unsafe fn async_enter(
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
        instance.prepare_call(
            StoreContextMut(self),
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

    unsafe fn async_exit(
        &mut self,
        instance: &mut ComponentInstance,
        callback: *mut VMFuncRef,
        post_return: *mut VMFuncRef,
        callee: *mut VMFuncRef,
        param_count: u32,
        result_count: u32,
        flags: u32,
    ) -> Result<u32> {
        instance.start_call(
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

    unsafe fn future_write(
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
        // SAFETY: This function is only called from
        // `wasmtime-cranelift`-generated guest code, which ensures that
        // `instance` belongs to `self`, to which we have (and may confer)
        // exclusive access in the following call.
        unsafe {
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
    }

    unsafe fn future_read(
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
        // SAFETY: See corresponding comment in `Self::future_write`.
        unsafe {
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
    }

    unsafe fn stream_write(
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
        // SAFETY: See corresponding comment in `Self::future_write`.
        unsafe {
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
    }

    unsafe fn stream_read(
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
        // SAFETY: See corresponding comment in `Self::future_write`.
        unsafe {
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
    }

    unsafe fn flat_stream_write(
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
        // SAFETY: See corresponding comment in `Self::future_write`.
        unsafe {
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
    }

    unsafe fn flat_stream_read(
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
        // SAFETY: See corresponding comment in `Self::future_write`.
        unsafe {
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
    }

    unsafe fn error_context_debug_message(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        err_ctx_handle: u32,
        debug_msg_address: u32,
    ) -> Result<()> {
        // SAFETY: See corresponding comment in `Self::future_write`.
        unsafe {
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
}

enum HostTaskOutput {
    Result(Result<()>),
    Function(Box<dyn FnOnce(&mut dyn VMStore, &mut ComponentInstance) -> Result<()> + Send>),
}

impl HostTaskOutput {
    fn consume(self, store: &mut dyn VMStore, instance: &mut ComponentInstance) -> Result<()> {
        match self {
            Self::Function(fun) => fun(store, instance),
            Self::Result(result) => result,
        }
    }
}

type HostTaskFuture = Pin<Box<dyn Future<Output = HostTaskOutput> + Send + 'static>>;

struct HostTask {
    common: WaitableCommon,
    caller_instance: RuntimeComponentInstanceIndex,
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
            abort_handle,
        }
    }
}

impl TableDebug for HostTask {
    fn type_name() -> &'static str {
        "HostTask"
    }
}

type CallbackFn = Box<
    dyn Fn(
            &mut dyn VMStore,
            &mut ComponentInstance,
            RuntimeComponentInstanceIndex,
            Event,
            u32,
        ) -> Result<u32>
        + Send
        + Sync
        + 'static,
>;

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
    callback: Option<CallbackFn>,
    caller: Caller,
    call_context: Option<CallContext>,
    sync_result: Option<Option<ValRaw>>,
    cancel_sent: bool,
    starting_sent: bool,
    context: [u32; 2],
    subtasks: HashSet<TableId<GuestTask>>,
    sync_call_set: TableId<WaitableSet>,
    instance: RuntimeComponentInstanceIndex,
    event: Option<Event>,
    waiting_on: Option<TableId<WaitableSet>>,
}

impl GuestTask {
    fn new(
        instance: &mut ComponentInstance,
        lower_params: RawLower,
        lift_result: LiftResult,
        caller: Caller,
        callback: Option<CallbackFn>,
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
            call_context: Some(CallContext::default()),
            sync_result: None,
            cancel_sent: false,
            starting_sent: false,
            context: [0u32; 2],
            subtasks: HashSet::new(),
            sync_call_set,
            instance: component_instance,
            event: None,
            waiting_on: None,
        })
    }

    fn dispose(self, instance: &mut ComponentInstance, me: TableId<GuestTask>) -> Result<()> {
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

impl TableDebug for GuestTask {
    fn type_name() -> &'static str {
        "GuestTask"
    }
}

#[derive(Default)]
struct WaitableCommon {
    event: Option<Event>,
    set: Option<TableId<WaitableSet>>,
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

    fn common<'a>(&self, instance: &'a mut ComponentInstance) -> Result<&'a mut WaitableCommon> {
        Ok(match self {
            Self::Host(id) => &mut instance.get_mut(*id)?.common,
            Self::Guest(id) => &mut instance.get_mut(*id)?.common,
            Self::Transmit(id) => &mut instance.get_mut(*id)?.common,
        })
    }

    fn set_event(&self, instance: &mut ComponentInstance, event: Option<Event>) -> Result<()> {
        log::trace!("set event for {self:?}: {event:?}");
        self.common(instance)?.event = event;
        self.mark_ready(instance)
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
            if let Some((task, mode)) = instance.get_mut(set)?.waiting.pop_first() {
                let waiting_on = instance.get_mut(task)?.waiting_on.take();
                assert!(waiting_on.is_none() || waiting_on == Some(set));

                let item = match mode {
                    WaitMode::Fiber(fiber) => WorkItem::ResumeFiber(fiber),
                    WaitMode::Callback(instance) => WorkItem::GuestCall(GuestCall {
                        task,
                        kind: GuestCallKind::DeliverEvent {
                            instance,
                            set: Some(set),
                        },
                    }),
                };
                instance.push_high_priority(item);
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
                log::trace!("delete host task {task:?}");
                instance.delete(*task)?;
            }
            Self::Guest(task) => {
                log::trace!("delete guest task {task:?}");
                instance.delete(*task)?.dispose(instance, *task)?;
            }
            Self::Transmit(task) => {
                instance.delete(*task)?;
            }
        }

        Ok(())
    }
}

impl fmt::Debug for Waitable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Host(id) => write!(f, "{id:?}"),
            Self::Guest(id) => write!(f, "{id:?}"),
            Self::Transmit(id) => write!(f, "{id:?}"),
        }
    }
}

#[derive(Default)]
struct WaitableSet {
    ready: BTreeSet<Waitable>,
    waiting: BTreeMap<TableId<GuestTask>, WaitMode>,
}

impl TableDebug for WaitableSet {
    fn type_name() -> &'static str {
        "WaitableSet"
    }
}

type RawLower = Box<
    dyn FnOnce(&mut dyn VMStore, &mut ComponentInstance, &mut [MaybeUninit<ValRaw>]) -> Result<()>
        + Send
        + Sync,
>;

pub type LowerFn<T> = unsafe fn(
    Func,
    StoreContextMut<T>,
    &mut ComponentInstance,
    *mut u8,
    &mut [MaybeUninit<ValRaw>],
) -> Result<()>;

type RawLift = Box<
    dyn FnOnce(
            &mut dyn VMStore,
            &mut ComponentInstance,
            &[ValRaw],
        ) -> Result<Box<dyn Any + Send + Sync>>
        + Send
        + Sync,
>;

pub type LiftFn<T> = fn(
    Func,
    StoreContextMut<T>,
    &mut ComponentInstance,
    &[ValRaw],
) -> Result<Box<dyn Any + Send + Sync>>;

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
    do_not_enter: bool,
    pending: BTreeMap<TableId<GuestTask>, GuestCallKind>,
}

pub struct ConcurrentState {
    guest_task: Option<TableId<GuestTask>>,
    futures: Mutex<Option<FuturesUnordered<HostTaskFuture>>>,
    table: Table,
    // TODO: this can and should be a `PrimaryMap`
    instance_states: HashMap<RuntimeComponentInstanceIndex, InstanceState>,
    waitable_tables: PrimaryMap<RuntimeComponentInstanceIndex, StateTable<WaitableState>>,
    high_priority: Vec<WorkItem>,
    low_priority: Vec<WorkItem>,
    suspend_reason: Option<SuspendReason>,
    worker: Option<StoreFiber<'static>>,
    guest_call: Option<GuestCall>,

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
            futures: Mutex::new(Some(FuturesUnordered::new())),
            instance_states: HashMap::new(),
            waitable_tables,
            high_priority: Vec::new(),
            low_priority: Vec::new(),
            suspend_reason: None,
            worker: None,
            guest_call: None,
            error_context_tables,
            global_error_context_ref_counts: BTreeMap::new(),
        }
    }

    pub fn drop_fibers(&mut self) {
        self.table = Table::new();
        self.worker = None;
        self.high_priority = Vec::new();
        self.low_priority = Vec::new();
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
    F: FnOnce(&mut dyn VMStore, &mut ComponentInstance, &mut [MaybeUninit<ValRaw>]) -> Result<()>
        + Send
        + Sync,
>(
    fun: F,
) -> F {
    fun
}

fn for_any_lift<
    F: FnOnce(
            &mut dyn VMStore,
            &mut ComponentInstance,
            &[ValRaw],
        ) -> Result<Box<dyn Any + Send + Sync>>
        + Send
        + Sync,
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
        INSTANCE_STATE.with(|v| {
            let matched = match v.get() {
                InstanceThreadLocalState::Detached {
                    instance: local, ..
                } => local.as_ptr() == instance.as_ptr(),
                InstanceThreadLocalState::Attached { instance: local } => {
                    local.0 == unsafe { (*instance.as_ptr()).instance.unwrap().0 }
                }
                InstanceThreadLocalState::None => false,
                InstanceThreadLocalState::Polling => unreachable!(),
            };

            if !matched {
                panic!("{message}")
            }
        });
        fut.as_mut().poll(cx)
    })
}

fn check_recursive_run() {
    INSTANCE_STATE.with(|v| {
        if !matches!(v.get(), InstanceThreadLocalState::None) {
            panic!("Recursive `Instance::run{{_with}}` calls not supported")
        }
    });
}

/// Run the specified function on a newly-created fiber and `.await` its
/// completion.
pub(crate) async fn on_fiber<R: Send + 'static, T: Send>(
    mut store: StoreContextMut<'_, T>,
    func: impl FnOnce(&mut StoreContextMut<T>) -> R + Send,
) -> Result<R> {
    // SAFETY: The returned future closes over `store`, so the borrow checker
    // will ensure the requirements of `on_fiber_raw` are met.
    let token = StoreToken::new(store.as_context_mut());
    unsafe {
        on_fiber_raw(VMStoreRawPtr(store.traitobj()), move |store| {
            func(&mut token.as_context_mut(&mut *store))
        })
        .await
    }
}

/// Same as `on_fiber`, but accepts a `&mut StoreOpaque` instead of a
/// `StoreContextMut`.
#[cfg(feature = "gc")]
pub(crate) async fn on_fiber_opaque<R: Send + 'static>(
    store: &mut StoreOpaque,
    func: impl FnOnce(&mut StoreOpaque) -> R + Send,
) -> Result<R> {
    // SAFETY: The returned future closes over `store`, so the borrow checker
    // will ensure the requirements of `on_fiber_raw` are met.
    unsafe {
        on_fiber_raw(VMStoreRawPtr(store.traitobj()), move |store| {
            func((*store).store_opaque_mut())
        })
        .await
    }
}

/// Wrap the specified function in a fiber and return it.
///
/// SAFETY: `store` must be a valid pointer to a store and remain so for as long
/// as the returned fiber exists.  Furthermore, the resuming thread(s) must
/// confer exclusive access to that fiber until it is either dropped, resolved,
/// or forgotten.
unsafe fn prepare_fiber<'a, R: Send + 'static>(
    store: VMStoreRawPtr,
    func: impl FnOnce(*mut dyn VMStore) -> R + Send + 'a,
) -> Result<(StoreFiber<'a>, oneshot::Receiver<R>)> {
    let (tx, rx) = oneshot::channel();
    let fiber = make_fiber(store, {
        move |store| {
            _ = tx.send(func(store));
            Ok(())
        }
    })?;
    Ok((fiber, rx))
}

/// Run the specified function on a newly-created fiber and `.await` its
/// completion.
///
/// SAFETY: `store` must be a valid pointer to a store and remain so for as long
/// as the returned future exists.  Furthermore, the polling thread(s) must
/// confer exclusive access to that future until it is either dropped, resolved,
/// or forgotten.
async unsafe fn on_fiber_raw<R: Send + 'static>(
    store: VMStoreRawPtr,
    func: impl FnOnce(*mut dyn VMStore) -> R + Send,
) -> Result<R> {
    let (mut fiber, mut rx) = prepare_fiber(store, func)?;

    let guard_range = fiber.guard_range();
    poll_fn(store, guard_range, move |_, mut store| {
        match resume_fiber(&mut fiber, store.take(), Ok(())) {
            Ok(Ok((store, result))) => Ok(result.map(|()| store)),
            Ok(Err(s)) => Err(s),
            Err(e) => Ok(Err(e)),
        }
    })
    .await?;

    Ok(rx.try_recv().unwrap().unwrap())
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
}

impl StoreFiber<'_> {
    fn guard_range(&self) -> (Option<SendSyncPtr<u8>>, Option<SendSyncPtr<u8>>) {
        self.fiber
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
            .unwrap_or((None, None))
    }
}

impl Drop for StoreFiber<'_> {
    fn drop(&mut self) {
        if !self.fiber.as_ref().unwrap().done() {
            // Safety: `self` is a valid pointer and we pass a `store`
            // parameter of `None` to inform the resumed fiber that it does
            // _not_ have access to the store.
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
    store: VMStoreRawPtr,
    fun: impl FnOnce(*mut dyn VMStore) -> Result<()> + 'a,
) -> Result<StoreFiber<'a>> {
    let engine = (*store.0.as_ptr()).engine().clone();
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
        suspend: (*store.0.as_ptr())
            .store_opaque_mut()
            .concurrent_async_state()
            .current_suspend
            .get(),
        stack_limit: (*store.0.as_ptr()).vm_store_context().stack_limit.get(),
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

unsafe fn resume_fiber(
    fiber: &mut StoreFiber,
    store: Option<*mut dyn VMStore>,
    result: Result<()>,
) -> Result<Result<(*mut dyn VMStore, Result<()>), Option<*mut dyn VMStore>>> {
    match resume_fiber_raw(fiber, store, result).map(|(store, result)| (store.unwrap(), result)) {
        Ok(pair) => Ok(Ok(pair)),
        Err(s) => {
            if let Some(range) = fiber.fiber.as_ref().unwrap().stack().range() {
                AsyncWasmCallState::assert_current_state_not_in_range(range);
            }

            Ok(Err(s))
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

pub(crate) struct ResetPtr<'a>(pub(crate) &'a AtomicPtr<u8>);

impl<'a> Drop for ResetPtr<'a> {
    fn drop(&mut self) {
        self.0.store(ptr::null_mut(), Relaxed);
    }
}

/// Drop the specified pointer as a `Box<T>`.
///
/// SAFETY: The specified pointer must be a valid `*mut T` which was originally
/// allocated as a `Box<T>`.
pub(crate) unsafe fn drop_params<T>(pointer: *mut u8) {
    drop(unsafe { Box::from_raw(pointer as *mut T) })
}

pub(crate) struct PreparedCall<R> {
    handle: Func,
    task: TableId<GuestTask>,
    param_count: usize,
    rx: oneshot::Receiver<LiftedResult>,
    params: Arc<AtomicPtr<u8>>,
    _phantom: PhantomData<R>,
}

impl<R> PreparedCall<R> {
    pub(crate) fn params(&self) -> &Arc<AtomicPtr<u8>> {
        &self.params
    }
}

/// Prepare a call to the specified exported Wasm function, providing functions
/// for lowering the parameters and lifting the result.
///
/// To enqueue the returned `PreparedCall` in the `ComponentInstance`'s event
/// loop, use `queue_call`.
///
/// Note that this function is used in `TypedFunc::call_async`, which accepts
/// parameters of a generic type which might not be `'static`.  However the
/// `GuestTask` created by this function must be `'static`, so it can't safely
/// close over those parameters.  Instead, `PreparedCall` has a `params` field
/// of type `Arc<AtomicPtr<u8>>`, which the caller is responsible for setting to
/// a valid, non-null pointer to the params prior to polling the event loop (at
/// least until the parameters have been lowered), and then resetting back to
/// null afterward.  That ensures that the lowering code never sees a stale
/// pointer, even if the application `drop`s or `mem::forget`s the future
/// returned by `TypedFunc::call_async`.
///
/// In the case where the parameters are passed using a type that _is_
/// `'static`, they can be boxed and stored in `PreparedCall::params`
/// indefinitely; `drop_params` will be called when they are no longer needed.
///
/// SAFETY: The `lower_params` and `drop_params` functions must accept (and
/// either ignore or safely use) any non-null pointer stored in the `params`
/// field of the returned `PreparedCall`, and that pointer must be valid for as
/// long as it is stored in that field.  Also, the `lower_params` and
/// `lift_result` functions must both use their other pointer arguments safely
/// or not at all.
pub(crate) unsafe fn prepare_call<T: Send + 'static, R>(
    mut store: StoreContextMut<T>,
    lower_params: LowerFn<T>,
    drop_params: unsafe fn(*mut u8),
    lift_result: LiftFn<T>,
    handle: Func,
    param_count: usize,
) -> Result<PreparedCall<R>> {
    let func_data = &store.0[handle.0];
    let task_return_type = func_data.types[func_data.ty].results;
    let component_instance = func_data.component_instance;
    let instance = func_data.instance;
    let callback = func_data.options.callback;
    let memory = func_data.options.memory.map(SendSyncPtr::new);
    let string_encoding = func_data.options.string_encoding();
    let token = StoreToken::new(store.as_context_mut());

    store.with_detached_instance(&instance, |_, instance| {
        let params = Arc::new(AtomicPtr::new(ptr::null_mut()));

        assert!(instance.guest_task().is_none());

        let (tx, rx) = oneshot::channel();

        struct DropParams {
            params: Arc<AtomicPtr<u8>>,
            dropper: unsafe fn(*mut u8),
        }

        impl Drop for DropParams {
            fn drop(&mut self) {
                let ptr = self.params.swap(ptr::null_mut(), Relaxed);
                if !ptr.is_null() {
                    // SAFETY: Per the contract of the `prepare_call`, `ptr`
                    // must be valid and `self.dropper` must either use it
                    // safely or not at all.
                    unsafe {
                        (self.dropper)(ptr);
                    }
                }
            }
        }

        let drop_params = DropParams {
            params: params.clone(),
            dropper: drop_params,
        };

        let task = GuestTask::new(
            instance,
            Box::new(for_any_lower({
                let param_ptr = params.clone();
                move |store, instance, params| {
                    let ptr = param_ptr.load(Relaxed);
                    let result = if ptr.is_null() {
                        // If we've reached here, it presumably means we were
                        // called via `TypedFunc::call_async` and the future was
                        // dropped or `mem::forget`ed by the caller, meaning we
                        // no longer have access to the parameters.  In that
                        // case, we should gracefully cancel the call without
                        // trapping or panicking.
                        todo!("gracefully cancel `call_async` tasks when future is dropped")
                    } else {
                        // SAFETY: This `ComponentInstance` belongs to the store
                        // in which it resides, so if it is valid then so is its
                        // store.  Furthermore, this closure is only called
                        // (transitively) from `ComponentInstance::poll_until`,
                        // which has exclusive access to both the
                        // `ComponentInstance` and the store.
                        //
                        // Also, per the contract of `prepare_call`, `ptr` must
                        // be valid and `lower_params` must either use it safely
                        // or not at all.
                        unsafe {
                            lower_params(handle, token.as_context_mut(store), instance, ptr, params)
                        }
                    };
                    drop(drop_params);
                    result
                }
            })),
            LiftResult {
                lift: Box::new(for_any_lift(move |store, instance, result| {
                    lift_result(handle, token.as_context_mut(store), instance, result)
                })),
                ty: task_return_type,
                memory,
                string_encoding,
            },
            Caller::Host(Some(tx)),
            callback.map(|callback| {
                let callback = SendSyncPtr::new(callback);
                Box::new(
                    move |store: &mut dyn VMStore,
                          instance: &mut ComponentInstance,
                          runtime_instance,
                          event,
                          handle| {
                        let store = token.as_context_mut(store);
                        unsafe {
                            instance.call_callback(store, runtime_instance, callback, event, handle)
                        }
                    },
                ) as CallbackFn
            }),
            component_instance,
        )?;

        let task = instance.push(task)?;

        Ok(PreparedCall {
            handle,
            task,
            param_count,
            rx,
            params,
            _phantom: PhantomData,
        })
    })
}

pub(crate) fn queue_call<T: 'static, R: Send + 'static>(
    mut store: StoreContextMut<T>,
    prepared: PreparedCall<R>,
) -> Result<impl Future<Output = Result<R>> + Send + 'static + use<T, R>> {
    let PreparedCall {
        handle,
        task,
        param_count,
        rx,
        ..
    } = prepared;

    start_call(store.as_context_mut(), handle, task, param_count)?;

    let func_data = &store[handle.0];
    let instance = func_data.instance;
    let instance_ptr = store.0[instance.0].as_ref().unwrap().instance_ptr();

    Ok(checked(
        SendSyncPtr::new(NonNull::new(instance_ptr).unwrap()),
        rx.map(|result| {
            result
                .map(|v| *v.downcast().unwrap())
                .map_err(anyhow::Error::from)
        }),
    ))
}

fn start_call<T: 'static>(
    mut store: StoreContextMut<T>,
    handle: Func,
    guest_task: TableId<GuestTask>,
    param_count: usize,
) -> Result<()> {
    let func_data = &store.0[handle.0];
    let is_concurrent = func_data.options.async_();
    let component_instance = func_data.component_instance;
    let instance = func_data.instance;
    let callee = func_data.export.func_ref;
    let callback = func_data.options.callback;
    let post_return = func_data.post_return;

    store.with_detached_instance(&instance, |store, instance| {
        log::trace!("starting call {guest_task:?}");

        *instance.guest_task() = Some(guest_task);
        log::trace!("pushed {guest_task:?} as current task; old task was None");

        instance.queue_call(
            store,
            guest_task,
            SendSyncPtr::new(callee),
            param_count,
            1,
            if callback.is_none() {
                None
            } else {
                Some(instance.instance_flags(component_instance))
            },
            is_concurrent,
            callback.map(SendSyncPtr::new),
            post_return.map(|f| SendSyncPtr::new(f.func_ref)),
        )?;

        *instance.guest_task() = None;
        log::trace!("popped current task {guest_task:?}; new task is None");

        log::trace!("started call {guest_task:?}");

        Ok(())
    })
}

/// Wrap the specified function in a future which, when polled, will store a
/// pointer to the `Context` in the `AsyncState::current_poll_cx` field for the
/// specified store and then call the function.
///
/// This is intended for use with functions that resume fibers which may need to
/// poll futures using the stored `Context` pointer.  The function should return
/// `Ok(_)` when complete, or `Err(_)` if the future should be polled again
/// later.
///
/// SAFETY: The `store` parameter must be a valid `*mut dyn VMStore` and `fun`
/// must use it safely.  The returned future must be polled to completion before
/// the store can be used by the caller again.  Finally, `fun` must not attempt
/// to use the `Context` pointer storied in `AsyncState::current_poll_cx` beyond
/// the scope of the current call.
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

        move |cx| {
            let _reset = Reset(poll_cx.0, *poll_cx.0);
            let guard_range_start = guard_range.0.map(|v| v.as_ptr()).unwrap_or(ptr::null_mut());
            let guard_range_end = guard_range.1.map(|v| v.as_ptr()).unwrap_or(ptr::null_mut());
            // SAFETY: We store the pointer to the `Context` only for the
            // duration of this call and then reset it to its previous value
            // afterward, thereby ensuring `fun` never sees a stale pointer.
            unsafe {
                *poll_cx.0 = PollContext {
                    future_context: mem::transmute::<&mut Context<'_>, *mut Context<'static>>(cx),
                    guard_range_start,
                    guard_range_end,
                };
            }
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
