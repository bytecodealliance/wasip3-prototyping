//! Runtime support for the Component Model Async ABI.
//!
//! This module and its submodules provide host runtime support for Component
//! Model Async features such as async-lifted exports, async-lowered imports,
//! streams, futures, and related intrinsics.  See [the Async
//! Explainer](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Async.md)
//! for a high-level overview.
//!
//! At the core of this support is an event loop which schedules and switches
//! between guest tasks and any host tasks they create.  Each
//! `ComponentInstance` will have at most one event loop running at any given
//! time, and that loop may be suspended and resumed by the host embedder using
//! e.g. `Instance::run`.  The `ComponentInstance::poll_until` function contains
//! the loop itself, while the `ComponentInstance::concurrent_state` field holds
//! its state.
//!
//! # Public API Overview
//!
//! ## Top-level API (e.g. kicking off host->guest calls and driving the event loop)
//!
//! - `[Typed]Func::call_concurrent`: Start a host->guest call to an
//! async-lifted or sync-lifted import, creating a guest task.
//!
//! - `Instance::run[_with]`: Run the event loop for the specified instance,
//! allowing any and all tasks belonging to that instance to make progress.
//!
//! - `Instance::spawn`: Run a background task as part of the event loop for the
//! specified instance.
//!
//! - `Instance::{future,stream}`: Create a new Component Model `future` or
//! `stream`; the read end may be passed to the guest.
//!
//! - `{Future,Stream}Reader::read` and `{Future,Stream}Writer::write`: read
//! from or write to a future or stream, respectively.
//!
//! ## Host Task API (e.g. implementing concurrent host functions and background tasks)
//!
//! - `LinkerInstance::func_wrap_concurrent`: Register a concurrent host
//! function with the linker.  That function will take an `Accessor` as its
//! first parameter, which provides access to the store and instance between
//! (but not across) await points.
//!
//! - `Accessor::with`: Access the store, its associated data, and the current
//! instance.
//!
//! - `Accessor::spawn`: Run a background task as part of the event loop for the
//! specified instance.  This is equivalent to `Instance::spawn` but more
//! convenient to use in host functions.

use {
    crate::{
        AsContext, AsContextMut, Engine, StoreContext, StoreContextMut, ValRaw,
        component::{
            HasData, HasSelf, Instance,
            func::{self, Func, Options},
        },
        store::{StoreInner, StoreOpaque, StoreToken},
        vm::{
            AsyncWasmCallState, PreviousAsyncWasmCallState, SendSyncPtr, VMFuncRef,
            VMMemoryDefinition, VMStore, VMStoreRawPtr,
            component::{CallContext, ComponentInstance, InstanceFlags, ResourceTables},
            mpk::{self, ProtectionMask},
        },
    },
    anyhow::{Context as _, Result, anyhow, bail},
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
        ops::Range,
        pin::{Pin, pin},
        ptr::{self, NonNull},
        sync::{
            Arc, Mutex,
            atomic::{AtomicPtr, Ordering::Relaxed},
        },
        task::{Context, Poll, Wake, Waker},
        vec::Vec,
    },
    table::{Table, TableDebug, TableError, TableId},
    wasmtime_environ::{
        PrimaryMap,
        component::{
            MAX_FLAT_PARAMS, MAX_FLAT_RESULTS, PREPARE_ASYNC_NO_RESULT, PREPARE_ASYNC_WITH_RESULT,
            RuntimeComponentInstanceIndex, StringEncoding,
            TypeComponentGlobalErrorContextTableIndex, TypeComponentLocalErrorContextTableIndex,
            TypeFutureTableIndex, TypeStreamTableIndex, TypeTupleIndex,
        },
        fact,
    },
    wasmtime_fiber::{Fiber, Suspend},
};

pub use futures_and_streams::{
    ErrorContext, FutureReader, FutureWriter, HostFuture, HostStream, ReadBuffer, StreamReader,
    StreamWriter, VecBuffer, Watch, WriteBuffer,
};
pub(crate) use futures_and_streams::{
    ResourcePair, lower_error_context_to_index, lower_future_to_index, lower_stream_to_index,
};

mod error_contexts;
mod futures_and_streams;
mod states;
mod table;

/// Constant defined in the Component Model spec to indicate that the async
/// intrinsic (e.g. `future.write`) has not yet completed.
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

/// Corresponds to `EventCode` in the Component Model spec, plus related payload
/// data.
#[derive(Clone, Copy, Debug)]
enum Event {
    None,
    Cancelled,
    Subtask {
        status: Status,
    },
    StreamRead {
        code: ReturnCode,
        pending: Option<(TypeStreamTableIndex, u32)>,
    },
    StreamWrite {
        code: ReturnCode,
        pending: Option<(TypeStreamTableIndex, u32)>,
    },
    FutureRead {
        code: ReturnCode,
        pending: Option<(TypeFutureTableIndex, u32)>,
    },
    FutureWrite {
        code: ReturnCode,
        pending: Option<(TypeFutureTableIndex, u32)>,
    },
}

impl Event {
    /// Lower this event to core Wasm integers for delivery to the guest.
    ///
    /// Note that the waitable handle, if any, is assumed to be lowered
    /// separately.
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

/// Corresponds to `CallbackCode` in the spec.
mod callback_code {
    pub const EXIT: u32 = 0;
    pub const YIELD: u32 = 1;
    pub const WAIT: u32 = 2;
    pub const POLL: u32 = 3;
}

/// A flag indicating that the callee is an async-lowered export.
///
/// This may be passed to the `async-start` intrinsic from a fused adapter.
const START_FLAG_ASYNC_CALLEE: u32 = fact::START_FLAG_ASYNC_CALLEE as u32;

/// Provides access to either store data (via the `get` method) or the store
/// itself (via [`AsContext`]/[`AsContextMut`]), as well as the component
/// instance to which the current host task belongs.
///
/// See [`Accessor::with`] for details.
pub struct Access<'a, T: 'static, D: HasData = HasSelf<T>> {
    accessor: &'a mut Accessor<T, D>,
    store: StoreContextMut<'a, T>,
}

impl<'a, T, D> Access<'a, T, D>
where
    D: HasData,
    T: 'static,
{
    /// Get mutable access to the store data.
    pub fn data_mut(&mut self) -> &mut T {
        self.store.data_mut()
    }

    /// Get mutable access to the store data.
    pub fn get(&mut self) -> D::Data<'_> {
        let get_data = self.accessor.get_data;
        get_data(self.data_mut())
    }

    /// Spawn a background task.
    ///
    /// See [`Accessor::spawn`] for details.
    pub fn spawn(&mut self, task: impl AccessorTask<T, D, Result<()>>) -> AbortHandle
    where
        T: 'static,
    {
        self.accessor.spawn(task)
    }

    /// Retrieve the component instance of the caller.
    pub fn instance(&self) -> Instance {
        self.accessor.instance()
    }
}

impl<'a, T, D> AsContext for Access<'a, T, D>
where
    D: HasData,
    T: 'static,
{
    type Data = T;

    fn as_context(&self) -> StoreContext<T> {
        self.store.as_context()
    }
}

impl<'a, T, D> AsContextMut for Access<'a, T, D>
where
    D: HasData,
    T: 'static,
{
    fn as_context_mut(&mut self) -> StoreContextMut<T> {
        self.store.as_context_mut()
    }
}

/// Provides scoped mutable access to store data in the context of a concurrent
/// host task future.
///
/// This allows multiple host task futures to execute concurrently and access
/// the store between (but not across) `await` points.
pub struct Accessor<T: 'static, D = HasSelf<T>>
where
    D: HasData,
{
    token: StoreToken<T>,
    get: fn() -> *mut dyn VMStore,
    get_data: fn(&mut T) -> D::Data<'_>,
    instance: Instance,
}

impl<T> Accessor<T> {
    /// Creates a new `Accessor` backed by the specified functions.
    ///
    /// - `get`: used to retrieve the store
    ///
    /// - `get_data`: used to "project" from the store's associated data to
    /// another type (e.g. a field of that data or a wrapper around it).
    ///
    /// - `spawn`: used to queue spawned background tasks to be run later
    ///
    /// - `instance`: used to access the `Instance` to which this `Accessor`
    /// (and the future which closes over it) belongs
    ///
    /// SAFETY: This relies on `get` either returning a valid `*mut dyn VMStore`
    /// whose data is of type `T` _or_ panicking if it is called outside its
    /// intended scope.  If it returns, the caller must be granted exclusive
    /// access to that store until the call to `Future::poll` for the current
    /// host task returns.
    unsafe fn new(token: StoreToken<T>, instance: Instance) -> Self {
        Self {
            token,
            get: get_store,
            get_data: |x| x,
            instance,
        }
    }
}

impl<T, D> Accessor<T, D>
where
    D: HasData,
{
    /// Run the specified closure, passing it mutable access to the store data.
    ///
    /// Note that the return value of the closure must be `'static`, meaning it
    /// cannot borrow from the store or its associated data.  If you need shared
    /// access to something in the store data, it must be cloned (using
    /// e.g. `Arc::clone` if appropriate).
    pub fn with<R: 'static>(&mut self, fun: impl FnOnce(Access<'_, T, D>) -> R) -> R {
        // SAFETY: Per the contract documented for `Accessor::new`, this will
        // either return exclusive access to the store or panic if it is somehow
        // called outside its intended scope.
        //
        // Note that, per the design of `Accessor::with`, the borrow checker
        // will ensure that the reference we return here cannot outlive the
        // scope of the closure passed to `Accessor::with` and thus cannot be
        // used beyond the current `Future::poll` call for the host task which
        // received the backing `Accessor`.
        //
        // TODO: something needs to prevent two `Accessor`s from using `with` at
        // the same time.
        let vmstore = unsafe { &mut *(self.get)() };
        fun(Access {
            store: self.token.as_context_mut(vmstore),
            accessor: self,
        })
    }

    /// TODO: is this safe? unsafe? should there be a lifetime in the
    /// returned value? no?
    #[doc(hidden)]
    pub unsafe fn with_data<D2: HasData>(
        &mut self,
        get_data: fn(&mut T) -> D2::Data<'_>,
    ) -> Accessor<T, D2> {
        Accessor {
            token: self.token,
            get: self.get,
            get_data,
            instance: self.instance,
        }
    }

    /// Spawn a background task which will receive an `&mut Accessor<T, D>` and
    /// run concurrently with any other tasks in progress for the current
    /// instance.
    ///
    /// This is particularly useful for host functions which return a `stream`
    /// or `future` such that the code to write to the write end of that
    /// `stream` or `future` must run after the function returns.
    ///
    /// The returned [`AbortHandle`] may be used to cancel the task.
    pub fn spawn(&mut self, task: impl AccessorTask<T, D, Result<()>>) -> AbortHandle
    where
        T: 'static,
    {
        let instance = self.instance;
        let accessor = Self {
            token: self.token,
            get: self.get,
            get_data: self.get_data,
            instance: self.instance,
        };
        self.with(|mut access| {
            instance.spawn_with_accessor(access.as_context_mut(), accessor, task)
        })
    }

    /// Retrieve the component instance of the caller.
    pub fn instance(&self) -> Instance {
        self.instance
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Wraps a future to allow it to be synchronously cancelled.
#[doc(hidden)]
pub enum AbortWrapper<T> {
    /// The future has not yet been polled
    Unpolled(BoxFuture<T>),
    /// The future has been polled at least once
    Polled { future: BoxFuture<T>, waker: Waker },
    /// The future has been cancelled
    Aborted,
}

impl<T> AbortWrapper<T> {
    /// Synchronously cancel the inner future by dropping it and notifying the
    /// waker if present.
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

/// Thread-local state for giving a host task future access to the store while
/// that future is being polled, plus a list of background tasks spawned by that
/// task, if any.
struct State {
    store: *mut dyn VMStore,
}

/// Thread-local state for making the store and component instance available to
/// futures polled as part of that instance's event loop.
///
/// This allows us to safely give those futures access to the store and
/// component instance between (but not across) `await` points, as well as
/// assert that any future which _must_ be polled as part of a specific
/// instance's event loop is indeed being polled that way.
#[derive(Copy, Clone)]
enum InstanceThreadLocalState {
    /// No instance's event loop is currently polling a future.
    None,
    /// An instance's event loop is currently polling a future, but the store
    /// and instance references have been temporarily taken out of the thread
    /// local state.
    Polling,
    /// The specified instance's event loop is currently polling a future, and
    /// the store and instance references may be accessed using
    /// the `with_local_instance` function.
    ///
    /// In this case, the store and instance are in a "detached" state, meaning
    /// they can be mutably referenced without either one aliasing the other.
    Detached {
        instance: SendSyncPtr<ComponentInstance>,
        handle: Instance,
        store: VMStoreRawPtr,
    },
    /// The specified instance's event loop is currently polling a future, and
    /// the instance handle is available via the `handle` field.
    ///
    /// In this case, the store and instance are in an "attached" state, meaning
    /// care must be taken to avoid creating mutable reference aliases.
    Attached { handle: Instance },
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
    /// See the `State` documentation.
    static STATE: RefCell<Option<State>> = RefCell::new(None);

    /// See the `InstanceThreadLocalState` documentation.
    static INSTANCE_STATE: Cell<InstanceThreadLocalState> = Cell::new(InstanceThreadLocalState::None);
}

/// Temporarily take exclusive access to the store and component instance state
/// from the thread-local state set when the instance's event loop polls a
/// future, passing them both to the specified function and returning the
/// result.
///
/// This will panic if `INSTANCE_STATE` does not match
/// `InstanceThreadLocalState::Detached { .. }` and thus should only be called
/// as a (transitive) child of `poll_with_local_instance`.
fn with_local_instance<R>(fun: impl FnOnce(&mut dyn VMStore, &mut ComponentInstance) -> R) -> R {
    let state = ResetInstanceThreadLocalState(
        INSTANCE_STATE.with(|v| v.replace(InstanceThreadLocalState::Polling)),
    );

    let InstanceThreadLocalState::Detached {
        instance, store, ..
    } = state.0
    else {
        unreachable!("expected `Detached`; got `{:?}`", state.0)
    };
    let (store, instance) = unsafe { (&mut *store.0.as_ptr(), &mut *instance.as_ptr()) };
    fun(store, instance)
}

/// Poll the specified future with the thread-local instance state pointing to
/// the specified store and instance.
///
/// The store and instance may be retrieved by (transitive) child calls using
/// `with_local_instance`.
fn poll_with_local_instance<F: Future + Send + ?Sized>(
    store: &mut dyn VMStore,
    instance: &mut ComponentInstance,
    future: &mut Pin<&mut F>,
    cx: &mut Context,
) -> Poll<F::Output> {
    let state = ResetInstanceThreadLocalState(INSTANCE_STATE.with(|v| {
        v.replace(InstanceThreadLocalState::Detached {
            instance: SendSyncPtr::new(instance.into()),
            handle: instance.instance,
            store: VMStoreRawPtr(store.into()),
        })
    }));

    assert!(matches!(state.0, InstanceThreadLocalState::None));

    future.as_mut().poll(cx)
}

/// Helper struct to reset the value of `STATE` to its previous value on drop.
struct ResetState(Option<State>);

impl Drop for ResetState {
    fn drop(&mut self) {
        STATE.with(|v| {
            *v.borrow_mut() = self.0.take();
        })
    }
}

/// Helper struct to reset the value of `INSTANCE_STATE` to its previous value on drop.
struct ResetInstanceThreadLocalState(InstanceThreadLocalState);

impl Drop for ResetInstanceThreadLocalState {
    fn drop(&mut self) {
        INSTANCE_STATE.with(|v| v.set(self.0))
    }
}

/// Retrieves the `State::store` field from `STATE`.
fn get_store() -> *mut dyn VMStore {
    STATE
        .with(|v| v.borrow().as_ref().map(|State { store, .. }| *store))
        .unwrap()
}

/// Poll the specified future using the store and instance references borrowed
/// using `with_local_instance`.
///
/// This will set the `STATE` thread-local variable to make the store available
/// to any `Accessor`s referenced by the future.  Additionally, it will spawn
/// any background tasks which may accumulate in `State::spawned` while the
/// future is being polled.
///
/// Note that this uses `with_local_instance` to access the thread-local store
/// and instance stored in `INSTANCE_STATE`.  See that function's documentation
/// for details.
fn poll_with_state<T: 'static, F: Future + ?Sized>(
    token: StoreToken<T>,
    cx: &mut Context,
    future: Pin<&mut F>,
) -> Poll<F::Output> {
    with_local_instance(|store, instance| {
        let store_ptr = store as *mut dyn VMStore;
        let mut store_cx = token.as_context_mut(store);

        let result = store_cx.with_attached_instance(instance, |_, _| {
            let old_state = STATE.with(|v| v.replace(Some(State { store: store_ptr })));
            let _reset_state = ResetState(old_state);
            future.poll(cx)
        });

        let spawned_tasks = mem::take(&mut store_cx.0.concurrent_async_state().spawned_tasks);
        for spawned in spawned_tasks {
            instance.push_future(spawned);
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

/// Represents parameter and result metadata for the caller side of a
/// guest->guest call orchestrated by a fused adapter.
enum CallerInfo {
    /// Metadata for a call to an async-lowered import
    Async {
        params: Vec<ValRaw>,
        has_result: bool,
    },
    /// Metadata for a call to an sync-lowered import
    Sync {
        params: Vec<ValRaw>,
        result_count: u32,
    },
}

/// Indicates how a guest task is waiting on a waitable set.
enum WaitMode {
    /// The guest task is waiting using `task.wait`
    Fiber(StoreFiber<'static>),
    /// The guest task is waiting via a callback declared as part of an
    /// async-lifted export.
    Callback(RuntimeComponentInstanceIndex),
}

/// Represents the reason a fiber is suspending itself.
#[derive(Debug)]
enum SuspendReason {
    /// The fiber is waiting for an event to be delivered to the specified
    /// waitable set or task.
    Waiting {
        set: TableId<WaitableSet>,
        task: TableId<GuestTask>,
    },
    /// The fiber has finished handling its most recent work item and is waiting
    /// for another (or to be dropped if it is no longer needed).
    NeedWork,
    /// The fiber is yielding and should be resumed once other tasks have had a
    /// chance to run.
    Yielding { task: TableId<GuestTask> },
}

/// Represents a pending call into guest code for a given guest task.
enum GuestCallKind {
    /// Indicates there's an event to deliver to the task, possibly related to a
    /// waitable set the task has been waiting on or polling.
    DeliverEvent {
        /// The (sub-)component instance in which the task has most recently
        /// been executing.
        ///
        /// Note that this might not be the same as the instance the guest task
        /// started executing in given that one or more synchronous guest->guest
        /// calls may have occurred involving multiple instances.
        instance: RuntimeComponentInstanceIndex,
        /// The waitable set the event belongs to, if any.
        ///
        /// If this is `None` the event will be waiting in the
        /// `GuestTask::event` field for the task.
        set: Option<TableId<WaitableSet>>,
    },
    /// Indicates that a new guest task call is pending and may be executed
    /// using the specified closure.
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

/// Represents a pending call into guest code for a given guest task.
#[derive(Debug)]
struct GuestCall {
    task: TableId<GuestTask>,
    kind: GuestCallKind,
}

impl GuestCall {
    /// Returns whether or not the call is ready to run.
    ///
    /// A call will not be ready to run if either:
    ///
    /// - the (sub-)component instance to be called has already been entered and
    /// cannot be reentered until an in-progress call completes
    ///
    /// - the call is for a not-yet started task and the (sub-)component
    /// instance to be called has backpressure enabled
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

/// Represents state related to an in-progress poll operation (e.g. `task.poll`
/// or `CallbackCode.POLL`).
#[derive(Debug)]
struct PollParams {
    /// Identifies the polling task.
    task: TableId<GuestTask>,
    /// The waitable set being polled.
    set: TableId<WaitableSet>,
    /// The (sub-)component instance in which the task has most recently been
    /// executing.
    ///
    /// Note that this might not be the same as the instance the guest task
    /// started executing in given that one or more synchronous guest->guest
    /// calls may have occurred involving multiple instances.
    instance: RuntimeComponentInstanceIndex,
}

/// Represents a pending work item to be handled by the event loop for a given
/// component instance.
enum WorkItem {
    /// A host task to be pushed to `ConcurrentState::futures`.
    PushFuture(Mutex<HostTaskFuture>),
    /// A fiber to resume.
    ResumeFiber(StoreFiber<'static>),
    /// A pending call into guest code for a given guest task.
    GuestCall(GuestCall),
    /// A pending `task.poll` or `CallbackCode.POLL` operation.
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
    /// Temporary code which will go away soon.  Nothing to see here, folks.
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
        // SAFETY: This isn't safe at all, since `fun` will easily be able to
        // create aliases of `instance` using
        // e.g. `StoreOpaque::component_instance_mut`.  As of this writing,
        // we're relying on `fun` never doing that except within a
        // `with_attached_instance` closure where it's sound.
        //
        // We're planning to remove `with_{a,de}tached_instance[_async]` soon
        // anyway and migrate the code which uses it to deal in `Instance`
        // parameters rather than `&mut ComponentInstance` parameters,
        // retrieving the latter only as necessary, with soundness enforced by
        // the borrow checker, at which point this will go away.
        let instance = unsafe { &mut *self.0[instance.0].as_mut().unwrap().instance_ptr() };
        fun(self.as_context_mut(), instance)
    }

    /// Temporary code which will go away soon.  Nothing to see here, folks.
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
        // SAFETY: See corresponding comment in `with_detached_instance`
        let instance = unsafe { &mut *self.0[instance.0].as_mut().unwrap().instance_ptr() };
        fun(self.as_context_mut(), instance).await
    }

    /// Temporary code which will go away soon.  Nothing to see here, folks.
    pub(crate) fn with_attached_instance<R>(
        &mut self,
        instance: &mut ComponentInstance,
        fun: impl FnOnce(StoreContextMut<'_, T>, Instance) -> R,
    ) -> R {
        let _state = ResetInstanceThreadLocalState(INSTANCE_STATE.with(|v| match v.get() {
            state @ InstanceThreadLocalState::None => state,
            InstanceThreadLocalState::Polling => v.replace(InstanceThreadLocalState::Attached {
                handle: instance.instance,
            }),
            _ => unreachable!(),
        }));
        fun(self.as_context_mut(), instance.instance)
    }
}

impl StoreOpaque {
    /// Temporary code which will go away soon.  Nothing to see here, folks.
    pub(crate) fn with_attached_instance<R>(
        &mut self,
        instance: &mut ComponentInstance,
        fun: impl FnOnce(&mut StoreOpaque, Instance) -> R,
    ) -> R {
        let _state = ResetInstanceThreadLocalState(INSTANCE_STATE.with(|v| match v.get() {
            state @ InstanceThreadLocalState::None => state,
            InstanceThreadLocalState::Polling => v.replace(InstanceThreadLocalState::Attached {
                handle: instance.instance,
            }),
            _ => unreachable!(),
        }));
        fun(self, instance.instance)
    }
}

impl ComponentInstance {
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
        // Note that we can't directly push to `ConcurrentState::futures` here
        // since this may be called from a future that's being polled inside
        // `Self::poll_until`, which temporarily removes the `FuturesUnordered`
        // so it has exclusive access while polling it.  Therefore, we push a
        // work item to the "high priority" queue, which will actually push to
        // `ConcurrentState::futures` later.
        self.push_high_priority(WorkItem::PushFuture(Mutex::new(future)));
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

    /// Push the call context for managing resource borrows for the specified
    /// guest task if it has not yet either returned a result or cancelled
    /// itself.
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

    /// Pop the call context for managing resource borrows for the specified
    /// guest task if it has not yet either returned a result or cancelled
    /// itself.
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

    /// Determine whether the instance associated with the specified guest task
    /// may be entered (i.e. is not already on the async call stack).
    ///
    /// This is an additional check on top of the "may_enter" instance flag;
    /// it's needed because async-lifted exports with callback functions must
    /// not call their own instances directly or indirectly, and due to the
    /// "stackless" nature of callback-enabled guest tasks this may happen even
    /// if there are no activation records on the stack (i.e. the "may_enter"
    /// field is `true`) for that instance.
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

    /// Handle the `CallbackCode` returned from an async-lifted export or its
    /// callback.
    ///
    /// If `initial_call` is `true`, then the code was received from the
    /// async-lifted export; otherwise, it was received from its callback.
    fn handle_callback_code(
        &mut self,
        guest_task: TableId<GuestTask>,
        runtime_instance: RuntimeComponentInstanceIndex,
        code: u32,
        initial_call: bool,
    ) -> Result<()> {
        let (code, set) = unpack_callback_code(code);

        log::trace!("received callback code from {guest_task:?}: {code} (set: {set})");

        let task = self.get_mut(guest_task)?;

        if task.lift_result.is_some() {
            if code == callback_code::EXIT {
                return Err(anyhow!(crate::Trap::NoAsyncResult));
            }
            if initial_call {
                // Notify any current or future waiters that this subtask has
                // started.
                Waitable::Guest(guest_task).set_event(
                    self,
                    Some(Event::Subtask {
                        status: Status::Started,
                    }),
                )?;
            }
        }

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
                // Push this task onto the "low priority" queue so it runs after
                // any other tasks have had a chance to run.
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
                    // An event is immediately available; deliver it ASAP.
                    self.push_high_priority(WorkItem::GuestCall(GuestCall {
                        task: guest_task,
                        kind: GuestCallKind::DeliverEvent {
                            instance: runtime_instance,
                            set: Some(set),
                        },
                    }));
                } else {
                    // No event is immediately available.
                    match code {
                        callback_code::POLL => {
                            // We're polling, so just yield and check whether an
                            // event has arrived after that.
                            self.push_low_priority(WorkItem::Poll(PollParams {
                                task: guest_task,
                                instance: runtime_instance,
                                set,
                            }));
                        }
                        callback_code::WAIT => {
                            // We're waiting, so register to be woken up when an
                            // event is published for this waitable set.
                            //
                            // Here we also set `GuestTask::wake_on_cancel`
                            // which allows `subtask.cancel` to interrupt the
                            // wait.
                            let old = self.get_mut(guest_task)?.wake_on_cancel.replace(set);
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

    /// Record that we're about to enter a (sub-)component instance which does
    /// not support more than one concurrent, stackful activation, meaning it
    /// cannot be entered again until the next call returns.
    fn enter_instance(&mut self, instance: RuntimeComponentInstanceIndex) {
        self.instance_state(instance).do_not_enter = true;
    }

    /// Record that we've exited a (sub-)component instance previously entered
    /// with `Self::enter_instance` and then calls `Self::partition_pending`.
    /// See the documentation for the latter for details.
    fn exit_instance(&mut self, instance: RuntimeComponentInstanceIndex) -> Result<()> {
        self.instance_state(instance).do_not_enter = false;
        self.partition_pending(instance)
    }

    /// Iterate over `InstanceState::pending`, moving any ready items into the
    /// "high priority" work item queue.
    ///
    /// See `GuestCall::is_ready` for details.
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

    /// Add the specified guest call to the "high priority" work item queue, to
    /// be started as soon as backpressure and/or reentrance rules allow.
    ///
    /// SAFETY: The raw pointer arguments must be valid references to guest
    /// functions (with the appropriate signatures) when the closures queued by
    /// this function are called.
    unsafe fn queue_call<T: 'static>(
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
        /// the returned closure is called.
        unsafe fn make_call<T: 'static>(
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

                // SAFETY: Per the contract documented in `make_call's`
                // documentation, `callee` must be a valid pointer.
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

        // SAFETY: Per the contract described in this function documentation,
        // the `callee` pointer which `call` closes over must be valid when
        // called by the closure we queue below.
        let call = unsafe {
            make_call(
                store.as_context_mut(),
                guest_task,
                callee,
                param_count,
                result_count,
                flags,
            )
        };

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
                    // Per the contract described in the `queue_call`
                    // documentation, the `callee` pointer which `call` closes
                    // over must be valid.
                    let storage = call(store, instance)?;

                    instance.exit_instance(callee_instance)?;

                    instance.maybe_pop_call_context(store.store_opaque_mut(), guest_task)?;

                    *instance.guest_task() = old_task;
                    log::trace!("stackless call: restored {old_task:?} as current task");

                    // SAFETY: `wasmparser` will have validated that the callback
                    // function returns a `i32` result.
                    let code = unsafe { storage[0].assume_init() }.get_i32() as u32;

                    instance.handle_callback_code(guest_task, callee_instance, code, true)?;

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

                    // Unless this is a callback-less (i.e. stackful)
                    // async-lifted export, we need to record that the instance
                    // cannot be entered until the call returns.
                    if !async_ {
                        instance.enter_instance(callee_instance);
                    }

                    // SAFETY: See the documentation for `make_call` to review the
                    // contract we must uphold for `call` here.
                    //
                    // Per the contract described in the `queue_call`
                    // documentation, the `callee` pointer which `call` closes
                    // over must be valid.
                    let storage = call(store, instance)?;

                    if async_ {
                        // This is a callback-less (i.e. stackful) async-lifted
                        // export, so there is no post-return function, and
                        // either `task.return` or `task.cancel` should have
                        // been called.
                        if instance.get(guest_task)?.lift_result.is_some() {
                            return Err(anyhow!(crate::Trap::NoAsyncResult));
                        }
                    } else {
                        // This is a sync-lifted export, so now is when we lift
                        // the result, call the post-return function, if any,
                        // and finally notify any current or future waiters that
                        // the subtask has returned.

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
    /// This is called from fused adapter code generated in
    /// `wasmtime_environ::fact::trampoline::Compiler`.  `start` and `return_`
    /// are synthesized Wasm functions which move the parameters from the caller
    /// to the callee and the result from the callee to the caller,
    /// respectively.  The adapter will call `Self::start_call` immediately
    /// after calling this function.
    ///
    /// SAFETY: All the pointer arguments must be valid pointers to guest
    /// entities (and with the expected signatures for the function references
    /// -- see `wasmtime_environ::fact::trampoline::Compiler` for details).
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
            CallerInfo::Async {
                has_result: true,
                params,
            } => ResultInfo::Heap {
                results: params.last().unwrap().get_u32(),
            },
            CallerInfo::Async {
                has_result: false, ..
            } => ResultInfo::Stack { result_count: 0 },
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

        // Create a new guest task for the call, closing over the `start` and
        // `return_` functions to lift the parameters and lower the result,
        // respectively.
        let start = SendSyncPtr::new(NonNull::new(start).unwrap());
        let return_ = SendSyncPtr::new(NonNull::new(return_).unwrap());
        let old_task = self.guest_task().take();
        let token = StoreToken::new(store);
        let new_task = GuestTask::new(
            self,
            Box::new(move |store, instance, dst| {
                let mut store = token.as_context_mut(store);
                assert!(dst.len() <= MAX_FLAT_PARAMS);
                let mut src = [MaybeUninit::uninit(); MAX_FLAT_PARAMS];
                let count = match caller_info {
                    // Async callers, if they have a result, use the last
                    // parameter as a return pointer so chop that off if
                    // relevant here.
                    CallerInfo::Async { params, has_result } => {
                        let params = &params[..params.len() - usize::from(has_result)];
                        for (param, src) in params.iter().zip(&mut src) {
                            src.write(*param);
                        }
                        params.len()
                    }

                    // Sync callers forward everything directly.
                    CallerInfo::Sync { params, .. } => {
                        for (param, src) in params.iter().zip(&mut src) {
                            src.write(*param);
                        }
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

        // Make the new task the current one so that `Self::start_call` knows
        // which one to start.
        *self.guest_task() = Some(guest_task);
        log::trace!("pushed {guest_task:?} as current task; old task was {old_task:?}");

        Ok(())
    }

    /// Call the specified callback function for an async-lifted export.
    ///
    /// SAFETY: `function` must be a valid reference to a guest function of the
    /// correct signature for a callback.
    unsafe fn call_callback<T>(
        &mut self,
        mut store: StoreContextMut<T>,
        callee_instance: RuntimeComponentInstanceIndex,
        function: SendSyncPtr<VMFuncRef>,
        event: Event,
        handle: u32,
    ) -> Result<u32> {
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
    /// This is called from fused adapter code generated in
    /// `wasmtime_environ::fact::trampoline::Compiler`.  The adapter will call
    /// this function immediately after calling `Self::prepare_call`.
    ///
    /// SAFETY: The `*mut VMFuncRef` arguments must be valid pointers to guest
    /// functions with the appropriate signatures for the current guest task.
    /// If this is a call to an async-lowered import, the actual call may be
    /// deferred and run after this function returns, in which case the pointer
    /// arguments must also be valid when the call happens.
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
            // We're calling an async-lifted export with a callback, so store
            // the callback and related context as part of the task so we can
            // call it later when needed.
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
            // As of this writing, `start_call` is only used for guest->guest
            // calls.
            unreachable!()
        };
        let caller = *caller;
        let caller_instance = *instance;

        let callee_instance = task.instance;

        // Queue the call as a "high priority" work item.
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
            (flags & START_FLAG_ASYNC_CALLEE) != 0,
            NonNull::new(callback).map(SendSyncPtr::new),
            NonNull::new(post_return).map(SendSyncPtr::new),
        )?;

        // Use the caller's `GuestTask::sync_call_set` to register interest in
        // the subtask...
        let set = self.get_mut(caller)?.sync_call_set;
        Waitable::Guest(guest_task).join(self, Some(set))?;

        // ... and suspend this fiber temporarily while we wait for it to start.
        //
        // Note that we _could_ call the callee directly using the current fiber
        // rather than suspend this one, but that would make reasoning about the
        // event loop more complicated and is probably only worth doing if
        // there's a measurable performance benefit.  In addition, it would mean
        // blocking the caller if the callee calls a blocking sync-lowered
        // import.
        //
        // Alternatively, the fused adapter code could be modified to call the
        // callee directly without calling a host-provided intrinsic at all (in
        // which case it would need to do its own, inline backpressure checks,
        // etc.).  Again, we'd want to see a measurable performance benefit
        // before committing to such an optimization.
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
                // It returned, so we can stop waiting.
                break (status, None);
            } else if async_caller {
                // It hasn't returned yet, but the caller is calling via an
                // async-lowered import, so we generate a handle for the task
                // waitable and return the status.
                break (
                    status,
                    Some(
                        self.waitable_tables()[caller_instance]
                            .insert(guest_task.rep(), WaitableState::GuestTask)?,
                    ),
                );
            } else {
                // The callee hasn't returned yet, and the caller is calling via
                // a sync-lowered import, so we loop and keep waiting until the
                // callee returns.
            }
        };

        Waitable::Guest(guest_task).join(self, None)?;

        if let Some(storage) = storage {
            // The caller used a sync-lowered import to call an async-lifted
            // export, in which case the result, if any, has been stashed in
            // `GuestTask::sync_result`.
            if let Some(result) = self.get_mut(guest_task)?.sync_result.take() {
                if let Some(result) = result {
                    storage[0] = MaybeUninit::new(result);
                }
            } else {
                // This means the callee failed to call either `task.return` or
                // `task.cancel` before exiting.
                return Err(anyhow!(crate::Trap::NoAsyncResult));
            }
        }

        // Reset the current task to point to the caller as it resumes control.
        *self.guest_task() = Some(caller);
        log::trace!("popped current task {guest_task:?}; new task is {caller:?}");

        Ok(status.pack(waitable))
    }

    /// Wrap the specified host function in a future which will call it, passing
    /// it an `&mut Accessor<T>`.
    ///
    /// See the `Accessor` documentation for details.
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
        let token = StoreToken::new(store);
        // SAFETY: The `get_store` function we pass here is backed by a
        // thread-local variable which `poll_with_state` will populate and reset
        // with valid pointers to the store data and the store itself each time
        // the returned future is polled, respectively.
        let mut accessor = unsafe { Accessor::new(token, self.instance) };
        let mut future = Box::pin(async move { closure(&mut accessor, params).await });
        Box::pin(future::poll_fn(move |cx| {
            poll_with_state(token, cx, future.as_mut())
        }))
    }

    /// Poll the specified future once on behalf of a guest->host call using an
    /// async-lowered import.
    ///
    /// If it returns `Ready`, return `Ok(None)`.  Otherwise, if it returns
    /// `Pending`, add it to the set of futures to be polled as part of this
    /// instance's event loop until it completes, and then return
    /// `Ok(Some(handle))` where `handle` is the waitable handle to return.
    ///
    /// Whether the future returns `Ready` immediately or later, the `lower`
    /// function will be used to lower the result, if any, into the guest caller's
    /// stack and linear memory unless the task has been cancelled.
    pub(crate) fn first_poll<T: 'static, R: Send + Sync + 'static>(
        &mut self,
        mut store: StoreContextMut<T>,
        future: impl Future<Output = Result<R>> + Send + 'static,
        caller_instance: RuntimeComponentInstanceIndex,
        lower: impl FnOnce(StoreContextMut<T>, &mut ComponentInstance, R) -> Result<()> + Send + 'static,
    ) -> Result<Option<u32>> {
        let caller = self.guest_task().unwrap();
        // Wrap the future in an `AbortWrapper` so the guest can cancel it if
        // desired.
        let wrapped = Arc::new(Mutex::new(AbortWrapper::Unpolled(Box::pin(future))));
        // We create a new host task even though it might complete immediately
        // (in which case we won't need to pass a waitable back to the guest).
        // If it does complete immediately, we'll remove it before we return.
        let task = self.push(HostTask::new(
            caller_instance,
            Some(AbortHandle::new(wrapped.clone())),
        ))?;
        let token = StoreToken::new(store.as_context_mut());

        // Now we wrap the `AbortWrapper` in a `poll_fn` which checks whether it
        // has been cancelled, in which case we'll return `Poll::Ready`
        // immediately.  Otherwise, we'll poll the inner future.
        let future = future::poll_fn({
            let mut call_context = None;
            move |cx| {
                let mut wrapped = wrapped.try_lock().unwrap();

                let inner = mem::replace(&mut *wrapped, AbortWrapper::Aborted);
                if let AbortWrapper::Unpolled(mut future)
                | AbortWrapper::Polled { mut future, .. } = inner
                {
                    // Push the call context for managing any resource borrows
                    // for the task.
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

                    *wrapped = AbortWrapper::Polled {
                        future,
                        waker: cx.waker().clone(),
                    };

                    match result {
                        Poll::Ready(output) => Poll::Ready(Some(output)),
                        Poll::Pending => {
                            // Pop the call context for managing any resource
                            // borrows for the task.
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

        // Map the output of the future to a `HostTaskOutput` responsible for
        // lowering the result into the guest's stack and memory, as well as
        // notifying any waiters that the task returned.
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

        // Finally, poll the future.  We can use a dummy `Waker` here because
        // we'll add the future to `ConcurrentState::futures` and poll it
        // automatically from the event loop if it doesn't complete immediately
        // here.
        let poll = poll_with_local_instance(
            store.0.traitobj_mut(),
            self,
            &mut future.as_mut(),
            &mut Context::from_waker(&dummy_waker()),
        );

        Ok(match poll {
            Poll::Ready(output) => {
                // It finished immediately; lower the result and delete the
                // task.
                output.consume(store.0.traitobj_mut(), self)?;
                log::trace!("delete host task {task:?} (already ready)");
                self.delete(task)?;
                None
            }
            Poll::Pending => {
                // It hasn't finished yet; add the future to
                // `ConcurrentState::futures` so it will be polled by the event
                // loop and allocate a waitable handle to return to the guest.
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

    /// Poll the specified future until it completes on behalf of a guest->host
    /// call using a sync-lowered import.
    ///
    /// This is similar to `Self::first_poll` except it's for sync-lowered
    /// imports, meaning we don't need to handle cancellation and we can block
    /// the caller until the task completes, at which point the caller can
    /// handle lowering the result to the guest's stack and linear memory.
    pub(crate) fn poll_and_block<R: Send + Sync + 'static>(
        &mut self,
        store: &mut dyn VMStore,
        future: impl Future<Output = Result<R>> + Send + 'static,
        caller_instance: RuntimeComponentInstanceIndex,
    ) -> Result<R> {
        // If there is no current guest task set, that means the host function
        // was registered using e.g. `LinkerInstance::func_wrap`, in which case
        // it should complete immediately.
        let Some(caller) = *self.guest_task() else {
            return match pin!(future).poll(&mut Context::from_waker(&dummy_waker())) {
                Poll::Ready(result) => result,
                Poll::Pending => {
                    unreachable!()
                }
            };
        };

        // Save any existing result stashed in `GuestTask::result` so we can
        // replace it with the new result.
        let old_result = self
            .get_mut(caller)
            .with_context(|| format!("bad handle: {caller:?}"))?
            .result
            .take();

        // Add a temporary host task into the table so we can track its
        // progress.  Note that we'll never allocate a waitable handle for the
        // guest since we're being called synchronously.
        let task = self.push(HostTask::new(caller_instance, None))?;

        log::trace!("new host task child of {caller:?}: {task:?}");

        // Map the output of the future to a `HostTaskOutput` which will take
        // care of stashing the result in `GuestTask::result` and resuming this
        // fiber when the host task completes.
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

        // Finally, poll the future.  We can use a dummy `Waker` here because
        // we'll add the future to `ConcurrentState::futures` and poll it
        // automatically from the event loop if it doesn't complete immediately
        // here.
        let poll = poll_with_local_instance(
            store.traitobj_mut(),
            self,
            &mut future.as_mut(),
            &mut Context::from_waker(&dummy_waker()),
        );

        match poll {
            Poll::Ready(output) => {
                // It completed immediately; run the `HostTaskOutput` function
                // to stash the result and delete the task.
                output.consume(store, self)?;
                log::trace!("delete host task {task:?} (already ready)");
                self.delete(task)?;
            }
            Poll::Pending => {
                // It did not complete immediately; add it to
                // `ConcurrentState::futures` so it will be polled via the event
                // loop, then use `GuestTask::sync_call_set` to wait for the
                // task to complete, suspending the current fiber until it does
                // so.
                self.push_future(future);

                let set = self.get_mut(caller)?.sync_call_set;
                Waitable::Host(task).join(self, Some(set))?;

                self.suspend(store, SuspendReason::Waiting { set, task: caller })?;
            }
        }

        // Retrieve and return the result.
        Ok(*mem::replace(&mut self.get_mut(caller)?.result, old_result)
            .unwrap()
            .downcast()
            .unwrap())
    }

    /// Run this instance's event loop.
    ///
    /// The returned future will resolve when either the specified future
    /// completes (in which case we return its result) or no further progress
    /// can be made (in which case we trap with `Trap::AsyncDeadlock`).
    async fn poll_until<T, R: Send + Sync + 'static>(
        &mut self,
        store: StoreContextMut<'_, T>,
        future: impl Future<Output = R> + Send,
    ) -> Result<R> {
        let mut future = pin!(future);

        loop {
            // Take `ConcurrentState::futures` out of `self` so we can poll it
            // while also safely giving any of the futures inside access to
            // `self`.
            let mut futures = self
                .concurrent_state
                .futures
                .get_mut()
                .unwrap()
                .take()
                .unwrap();
            let mut next = pin!(futures.next());

            let result = future::poll_fn(|cx| {
                // First, poll the future we were passed as an argument and
                // return immediately if it's ready.
                if let Poll::Ready(value) =
                    poll_with_local_instance(store.0.traitobj_mut(), self, &mut future, cx)
                {
                    return Poll::Ready(Ok(Either::Left(value)));
                }

                // Next, poll `ConcurrentState::futures` (which includes any
                // pending host tasks and/or background tasks), returning
                // immediately if one of them fails.
                let next =
                    match poll_with_local_instance(store.0.traitobj_mut(), self, &mut next, cx) {
                        Poll::Ready(Some(output)) => {
                            if let Err(e) = output.consume(store.0.traitobj_mut(), self) {
                                return Poll::Ready(Err(e));
                            }
                            Poll::Ready(true)
                        }
                        Poll::Ready(None) => Poll::Ready(false),
                        Poll::Pending => Poll::Pending,
                    };

                // Next, check the "high priority" work queue and return
                // immediately if it has at least one item.
                let ready = mem::take(&mut self.concurrent_state.high_priority);
                let ready = if ready.is_empty() {
                    // Next, check the "low priority" work queue and return
                    // immediately if it has at least one item.
                    let ready = mem::take(&mut self.concurrent_state.low_priority);
                    if ready.is_empty() {
                        return match next {
                            // In this case, one of the futures in
                            // `ConcurrentState::futures` completed
                            // successfully, so we return now and continue the
                            // outer loop in case there is another one ready to
                            // complete.
                            Poll::Ready(true) => Poll::Ready(Ok(Either::Right(Vec::new()))),
                            // In this case, there are no more pending futures
                            // in `ConcurrentState::futures`, there are no
                            // remaining work items, _and_ the future we were
                            // passed as an argument still hasn't completed,
                            // meaning we're stuck, so we return an error.  The
                            // underlying assumption is that `future` depends on
                            // this component instance making such progress, and
                            // thus there's no point in continuing to poll it
                            // given we've run out of work to do.
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
                            // There is at least one pending future in
                            // `ConcurrentState::futures` and we have nothing
                            // else to do but wait for now, so we return
                            // `Pending`.
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

            // Put the `ConcurrentState::futures` back into `self` before we
            // return or handle any work items since one or more of those items
            // might append more futures.
            *self.concurrent_state.futures.get_mut().unwrap() = Some(futures);

            match result? {
                // The future we were passed as an argument completed, so we
                // return the result.
                Either::Left(value) => break Ok(value),
                // The future we were passed has not yet completed, so handle
                // any work items and then loop again.
                Either::Right(ready) => {
                    for item in ready {
                        // SAFETY: We have exclusive access to the store and may
                        // grant it to the `handle_work_item` future until it
                        // completes.
                        unsafe {
                            self.handle_work_item(VMStoreRawPtr(store.0.traitobj()), item)
                                .await
                        }?;
                    }
                }
            }
        }
    }

    /// Get the next pending event for the specified task and (optional)
    /// waitable set, along with the waitable handle if applicable.
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

    /// Execute the specified guest call.
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

                self.handle_callback_code(call.task, instance, code, false)?;

                *self.guest_task() = old_task;
                log::trace!("GuestCallKind::DeliverEvent: restored {old_task:?} as current task");
            }
            GuestCallKind::Start(fun) => {
                fun(store, self)?;
            }
        }

        Ok(())
    }

    /// Resume the specified fiber, giving it exclusive access to the specified
    /// store.
    ///
    /// SAFETY: The caller must confer exclusive access to the store until the
    /// returned future completes.
    async unsafe fn resume_fiber(
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

        // SAFETY: Per the documented contract for this function, we have
        // exclusive access to the store.
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
            // See the `SuspendReason` documentation for what each case means.
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

    /// Handle the specified work item, possibly resuming a fiber if applicable.
    ///
    /// SAFETY: The caller must confer exclusive access to the store until the
    /// returned future completes.
    async unsafe fn handle_work_item(
        &mut self,
        store: VMStoreRawPtr,
        item: WorkItem,
    ) -> Result<()> {
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
                // SAFETY: Per the documented contract for this function, we have
                // exclusive access to the store.
                unsafe { self.resume_fiber(store, fiber).await }?;
            }
            WorkItem::GuestCall(call) => {
                if call.is_ready(self)? {
                    // SAFETY: Per the documented contract for this function, we
                    // have exclusive access to the store.
                    unsafe { self.run_on_worker(store, call).await }?;
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
                    // There's at least one event immediately available; deliver
                    // it to the guest ASAP.
                    self.push_high_priority(WorkItem::GuestCall(GuestCall {
                        task: params.task,
                        kind: GuestCallKind::DeliverEvent {
                            instance: params.instance,
                            set: Some(params.set),
                        },
                    }));
                } else {
                    // There are no events immediately available; deliver
                    // `Event::None` to the guest.
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

    /// Execute the specified guest call on a worker fiber.
    ///
    /// SAFETY: The caller must confer exclusive access to the store until the
    /// returned future completes.
    async unsafe fn run_on_worker(&mut self, store: VMStoreRawPtr, call: GuestCall) -> Result<()> {
        let worker = if let Some(fiber) = self.worker().take() {
            fiber
        } else {
            // Here we smuggle the `ComponentInstance` pointer into the closure
            // so that the fiber can use it without upsetting the borrow
            // checker.
            //
            // SAFETY: We will only resume this fiber in either
            // `ComponentInstance::handle_work_item` or
            // `ComponentInstance::run_on_worker`, where we'll have exclusive
            // access to the same `ComponentInstance` and thus be able to grant
            // the same access to the fiber we're resuming.
            //
            // TODO: Consider adding `*mut ComponentInstance` parameters to
            // `StoreFiber`'s `suspend` and `resume` signatures to make this
            // handoff explicit.
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

        // SAFETY: Per the documented contract for this function, we have
        // exclusive access to the store.
        unsafe { self.resume_fiber(store, worker).await }
    }

    /// Suspend the current fiber, storing the reason in
    /// `ConcurrentState::suspend_reason` to indicate the conditions under which
    /// it should be resumed.
    ///
    /// See the `SuspendReason` documentation for details.
    fn suspend(&mut self, store: &mut dyn VMStore, reason: SuspendReason) -> Result<()> {
        log::trace!("suspend fiber: {reason:?}");

        // If we're yielding or waiting on behalf of a guest task, we'll need to
        // pop the call context which manages resource borrows before suspending
        // and then push it again once we've resumed.
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

    /// Implements the `task.return` intrinsic, lifting the result for the
    /// current guest task.
    ///
    /// SAFETY: The `memory` and `storage` pointers must be valid, and `storage`
    /// must contain at least `storage_len` items.
    pub(crate) unsafe fn task_return(
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

    /// Implements the `task.cancel` intrinsic.
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

    /// Complete the specified guest task (i.e. indicate that it has either
    /// returned a (possibly empty) result or cancelled itself).
    ///
    /// This will return any resource borrows and notify any current or future
    /// waiters that the task has completed.
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
            Waitable::Guest(guest_task).set_event(self, Some(Event::Subtask { status }))?;
        }

        Ok(())
    }

    /// Implements the `backpressure.set` intrinsic.
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
            // Backpressure was previously enabled and is now disabled; move any
            // newly-eligible guest calls to the "high priority" queue.
            self.partition_pending(caller_instance)?;
        }

        Ok(())
    }

    /// Implements the `waitable-set.new` intrinsic.
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

    /// Implements the `waitable-set.wait` intrinsic.
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

    /// Implements the `waitable-set.poll` intrinsic.
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

    /// Implements the `yield` intrinsic.
    pub(crate) fn yield_(&mut self, store: &mut dyn VMStore, async_: bool) -> Result<bool> {
        self.waitable_check(store, async_, WaitableCheck::Yield)
            .map(|_code| {
                // TODO: plumb cancellation to here
                false
            })
    }

    /// Helper function for the `waitable-set.wait`, `waitable-set.poll`, and
    /// `yield` intrinsics.
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

        // First, suspend this fiber, allowing any other tasks to run.
        self.suspend(store, SuspendReason::Yielding { task: guest_task })?;

        log::trace!("waitable check for {guest_task:?}; set {set:?}");

        let task = self.get(guest_task)?;

        if wait && task.callback.is_some() {
            bail!("cannot call `task.wait` from async-lifted export with callback");
        }

        // If we're waiting, and there are no events immediately available,
        // suspend the fiber until that changes.
        if wait {
            let set = set.unwrap();

            if task.event.is_none() && self.get(set)?.ready.is_empty() {
                let old = self.get_mut(guest_task)?.wake_on_cancel.replace(set);
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
            // Deliver any pending events to the guest and return.
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
            // TODO: Check `GuestTask::event` in case it contains
            // `Event::Cancelled`, in which case we'll need to return that to
            // the guest.
            WaitableCheck::Yield => Ok(0),
        };

        result
    }

    /// Implements the `waitable-set.drop` intrinsic.
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

        let set = self.delete(TableId::<WaitableSet>::new(rep))?;

        if !set.waiting.is_empty() {
            bail!("cannot drop waitable set with waiters");
        }

        Ok(())
    }

    /// Implements the `waitable.join` intrinsic.
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

    /// Implements the `subtask.drop` intrinsic.
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
                let task = self.get(id)?;
                if task.abort_handle.is_some() {
                    bail!("cannot drop a subtask which has not yet resolved");
                }
                (Waitable::Host(id), task.caller_instance)
            }
            WaitableState::GuestTask => {
                let id = TableId::<GuestTask>::new(rep);
                let task = self.get(id)?;
                if task.lift_result.is_some() {
                    bail!("cannot drop a subtask which has not yet resolved");
                }
                if let Caller::Guest { instance, .. } = &task.caller {
                    (Waitable::Guest(id), *instance)
                } else {
                    unreachable!()
                }
            }
            _ => bail!("invalid task handle: {task_id}"),
        };

        if waitable.take_event(self)?.is_some() {
            bail!("cannot drop a subtask with an undelivered event");
        }

        // Since waitables can neither be passed between instances nor forged,
        // this should never fail unless there's a bug in Wasmtime, but we check
        // here to be sure:
        assert_eq!(expected_caller_instance, caller_instance);
        log::trace!("subtask_drop {waitable:?} (handle {task_id})");
        Ok(())
    }

    /// Implements the `subtask.cancel` intrinsic.
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
        // Since waitables can neither be passed between instances nor forged,
        // this should never fail unless there's a bug in Wasmtime, but we check
        // here to be sure:
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
                task.lower_params = None;
                task.lift_result = None;

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
                if let Some(set) = task.wake_on_cancel.take() {
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

    /// Implements the `context.get` intrinsic.
    pub(crate) fn context_get(&mut self, slot: u32) -> Result<u32> {
        let task = self.guest_task().unwrap();
        let val = self.get(task)?.context[usize::try_from(slot).unwrap()];
        log::trace!("context_get {task:?} slot {slot} val {val:#x}");
        Ok(val)
    }

    /// Implements the `context.set` intrinsic.
    pub(crate) fn context_set(&mut self, slot: u32, val: u32) -> Result<()> {
        let task = self.guest_task().unwrap();
        log::trace!("context_set {task:?} slot {slot} val {val:#x}");
        self.get_mut(task)?.context[usize::try_from(slot).unwrap()] = val;
        Ok(())
    }
}

impl Instance {
    /// Poll the specified future as part of this instance's event loop until it
    /// yields a result _or_ no further progress can be maded.
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
    pub async fn run<F>(
        &self,
        mut store: impl AsContextMut<Data: Send>,
        fut: F,
    ) -> Result<F::Output>
    where
        F: Future + Send,
        F::Output: Send + Sync + 'static,
    {
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
    /// Like [`Self::run`], this will poll a specified future as part of this
    /// instance's event loop until it yields a result _or_ there are no more
    /// tasks to run.  Unlike [`Self::run`], the future may close over an
    /// [`Accessor`], which provides controlled access to the `Store` and its
    /// data.
    ///
    /// This enables a different control flow model than `Self::run` in that the
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
                let token = StoreToken::new(store);
                // SAFETY: See corresponding comment in `ComponentInstance::wrap_call`.
                let mut accessor = unsafe { Accessor::new(token, instance.instance) };
                let mut future = Box::pin(async move { fun(&mut accessor).await });
                Box::pin(future::poll_fn(move |cx| {
                    poll_with_state(token, cx, future.as_mut())
                }))
            })
    }

    /// Spawn a background task to run as part of this instance's event loop.
    ///
    /// The task will receive an `&mut Accessor<U>` and run concurrently with
    /// any other tasks in progress for the instance.
    ///
    /// Note that the task will only make progress if and when the event loop
    /// for this instance is run.
    ///
    /// The returned [`SpawnHandle`] may be used to cancel the task.
    pub fn spawn<U: 'static>(
        &self,
        mut store: impl AsContextMut<Data = U>,
        task: impl AccessorTask<U, HasSelf<U>, Result<()>>,
    ) -> AbortHandle {
        let mut store = store.as_context_mut();
        // SAFETY: TODO
        let accessor = unsafe { Accessor::new(StoreToken::new(store.as_context_mut()), *self) };
        self.spawn_with_accessor(store, accessor, task)
    }

    /// Internal implementation of `spawn` functions where a `store` is
    /// available along with an `Accessor`.
    fn spawn_with_accessor<T, D>(
        &self,
        mut store: StoreContextMut<T>,
        mut accessor: Accessor<T, D>,
        task: impl AccessorTask<T, D, Result<()>>,
    ) -> AbortHandle
    where
        T: 'static,
        D: HasData,
    {
        let mut store = store.as_context_mut();

        // Here a `future` is created with a connection to the `handle` being
        // returned such that if the `handle` is dropped then the future will be
        // dropped immediately.
        //
        // This `future` is then used to create a "host task" which is pushed
        // directly into `ComponentInstance` once it's all prepared.
        // Additionally `poll_fn` is used to hook all calls to `poll` and test
        // if the returned `AbortHandle` has been dropped yet.
        let future = Arc::new(Mutex::new(AbortWrapper::Unpolled(Box::pin(async move {
            task.run(&mut accessor).await
        }))));
        let handle = AbortHandle::new(future.clone());
        let token = StoreToken::new(store.as_context_mut());
        let spawned = future.clone();
        let future = Box::pin(future::poll_fn({
            move |cx| {
                let mut spawned = spawned.try_lock().unwrap();
                // Poll the inner future if present; otherwise it has been
                // cancelled, in which case we return `Poll::Ready` immediately.
                let inner = mem::replace(&mut *spawned, AbortWrapper::Aborted);
                if let AbortWrapper::Unpolled(mut future)
                | AbortWrapper::Polled { mut future, .. } = inner
                {
                    let result = poll_with_state(token, cx, future.as_mut());
                    *spawned = AbortWrapper::Polled {
                        future,
                        waker: cx.waker().clone(),
                    };
                    result.map(HostTaskOutput::Result)
                } else {
                    Poll::Ready(HostTaskOutput::Result(Ok(())))
                }
            }
        }));
        store.with_detached_instance(self, |_, instance| instance.push_future(future));

        handle
    }
}

/// Trait representing component model ABI async intrinsics and fused adapter
/// helper functions.
///
/// SAFETY (callers): Most of the methods in this trait accept raw pointers,
/// which must be valid for at least the duration of the call (and possibly for
/// as long as the relevant guest task exists, in the case of `*mut VMFuncRef`
/// pointers used for async calls).
pub trait VMComponentAsyncStore {
    /// A helper function for fused adapter modules involving calls where the
    /// one of the caller or callee is async.
    ///
    /// This helper is not used when the caller and callee both use the sync
    /// ABI, only when at least one is async is this used.
    unsafe fn prepare_call(
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
    unsafe fn sync_start(
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
    unsafe fn async_start(
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
impl<T: 'static> VMComponentAsyncStore for StoreInner<T> {
    unsafe fn prepare_call(
        &mut self,
        instance: &mut ComponentInstance,
        memory: *mut VMMemoryDefinition,
        start: *mut VMFuncRef,
        return_: *mut VMFuncRef,
        caller_instance: RuntimeComponentInstanceIndex,
        callee_instance: RuntimeComponentInstanceIndex,
        task_return_type: TypeTupleIndex,
        string_encoding: u8,
        result_count_or_max_if_async: u32,
        storage: *mut ValRaw,
        storage_len: usize,
    ) -> Result<()> {
        // SAFETY: The `wasmtime_cranelift`-generated code that calls
        // this method will have ensured that `storage` is a valid
        // pointer containing at least `storage_len` items.
        let params = unsafe { std::slice::from_raw_parts(storage, storage_len) }.to_vec();

        instance.prepare_call(
            StoreContextMut(self),
            start,
            return_,
            caller_instance,
            callee_instance,
            task_return_type,
            memory,
            string_encoding,
            match result_count_or_max_if_async {
                PREPARE_ASYNC_NO_RESULT => CallerInfo::Async {
                    params,
                    has_result: false,
                },
                PREPARE_ASYNC_WITH_RESULT => CallerInfo::Async {
                    params,
                    has_result: true,
                },
                result_count => CallerInfo::Sync {
                    params,
                    result_count,
                },
            },
        )
    }

    unsafe fn sync_start(
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
                START_FLAG_ASYNC_CALLEE,
                // SAFETY: The `wasmtime_cranelift`-generated code that calls
                // this method will have ensured that `storage` is a valid
                // pointer containing at least `storage_len` items.
                Some(unsafe { std::slice::from_raw_parts_mut(storage, storage_len) }),
            )
            .map(drop)
    }

    unsafe fn async_start(
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
        // SAFETY: Per the trait-level documentation for
        // `VMComponentAsyncStore`, all raw pointers passed to this function
        // must be valid.
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

/// Represents the output of a host task or background task.
enum HostTaskOutput {
    /// A plain result
    Result(Result<()>),
    /// A function to be run after the future completes (e.g. post-processing
    /// which requires access to the store and instance).
    Function(Box<dyn FnOnce(&mut dyn VMStore, &mut ComponentInstance) -> Result<()> + Send>),
}

impl HostTaskOutput {
    /// Retrieve the result of the host or background task, running the
    /// post-processing function if present.
    fn consume(self, store: &mut dyn VMStore, instance: &mut ComponentInstance) -> Result<()> {
        match self {
            Self::Function(fun) => fun(store, instance),
            Self::Result(result) => result,
        }
    }
}

type HostTaskFuture = Pin<Box<dyn Future<Output = HostTaskOutput> + Send + 'static>>;

/// Represents the state of a pending host task.
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

/// Represents the caller of a given guest task.
enum Caller {
    /// The host called the guest task.
    ///
    /// The `Sender`, if present, may be used to deliver the result.
    Host(Option<oneshot::Sender<LiftedResult>>),
    /// Another guest task called the guest task
    Guest {
        /// The id of the caller
        task: TableId<GuestTask>,
        /// The instance to use to enforce reentrance rules.
        ///
        /// Note that this might not be the same as the instance the caller task
        /// started executing in given that one or more synchronous guest->guest
        /// calls may have occurred involving multiple instances.
        instance: RuntimeComponentInstanceIndex,
    },
}

/// Represents a closure and related canonical ABI parameters required to
/// validate a `task.return` call at runtime and lift the result.
struct LiftResult {
    lift: RawLift,
    ty: TypeTupleIndex,
    memory: Option<SendSyncPtr<VMMemoryDefinition>>,
    string_encoding: StringEncoding,
}

/// Represents a pending guest task.
struct GuestTask {
    /// See `WaitableCommon`
    common: WaitableCommon,
    /// Closure to lower the parameters passed to this task.
    lower_params: Option<RawLower>,
    /// See `LiftResult`
    lift_result: Option<LiftResult>,
    /// A place to stash the type-erased lifted result if it can't be delivered
    /// immediately.
    result: Option<LiftedResult>,
    /// Closure to call the callback function for an async-lifted export, if
    /// provided.
    callback: Option<CallbackFn>,
    /// See `Caller`
    caller: Caller,
    /// A place to stash the call context for managing resource borrows while
    /// switching between guest tasks.
    call_context: Option<CallContext>,
    /// A place to stash the lowered result for a sync-to-async call until it
    /// can be returned to the caller.
    sync_result: Option<Option<ValRaw>>,
    /// Whether or not the task has been cancelled (i.e. whether the task is
    /// permitted to call `task.cancel`).
    cancel_sent: bool,
    /// Whether or not we've sent a `Status::Starting` event to any current or
    /// future waiters for this waitable.
    starting_sent: bool,
    /// Context-local state used to implement the `context.{get,set}`
    /// intrinsics.
    context: [u32; 2],
    /// Pending guest subtasks created by this task (directly or indirectly).
    ///
    /// This is used to re-parent subtasks which are still running when their
    /// parent task is disposed.
    subtasks: HashSet<TableId<GuestTask>>,
    /// Scratch waitable set used to watch subtasks during synchronous calls.
    sync_call_set: TableId<WaitableSet>,
    /// The instance to which the exported function for this guest task belongs.
    ///
    /// Note that the task may do a sync->sync call via a fused adapter which
    /// results in that task executing code in a different instance, and it may
    /// call host functions and intrinsics from that other instance.
    instance: RuntimeComponentInstanceIndex,
    /// If present, a pending `Event::None` or `Event::Cancelled` to be
    /// delivered to this task.
    event: Option<Event>,
    /// If present, indicates that the task is currently waiting on the
    /// specified set but may be cancelled and woken immediately.
    wake_on_cancel: Option<TableId<WaitableSet>>,
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
            wake_on_cancel: None,
        })
    }

    /// Dispose of this guest task, reparenting any pending subtasks to the
    /// caller.
    fn dispose(self, instance: &mut ComponentInstance, me: TableId<GuestTask>) -> Result<()> {
        // If there are not-yet-delivered completion events for subtasks in
        // `self.sync_call_set`, recursively dispose of those subtasks as well.
        for waitable in mem::take(&mut instance.get_mut(self.sync_call_set)?.ready) {
            if let Some(Event::Subtask {
                status: Status::Returned | Status::ReturnCancelled,
            }) = waitable.common(instance)?.event
            {
                waitable.delete_from(instance)?;
            }
        }

        instance.delete(self.sync_call_set)?;

        // Reparent any pending subtasks to the caller.
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

/// Represents state common to all kinds of waitables.
#[derive(Default)]
struct WaitableCommon {
    /// The currently pending event for this waitable, if any.
    event: Option<Event>,
    /// The set to which this waitable belongs, if any.
    set: Option<TableId<WaitableSet>>,
}

/// Represents a Component Model Async `waitable`.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
enum Waitable {
    /// A host task
    Host(TableId<HostTask>),
    /// A guest task
    Guest(TableId<GuestTask>),
    /// The read or write end of a stream or future
    Transmit(TableId<TransmitHandle>),
}

impl Waitable {
    /// Retrieve the `Waitable` corresponding to the specified guest-visible
    /// handle.
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

    /// Retrieve the host-visible identifier for this `Waitable`.
    fn rep(&self) -> u32 {
        match self {
            Self::Host(id) => id.rep(),
            Self::Guest(id) => id.rep(),
            Self::Transmit(id) => id.rep(),
        }
    }

    /// Move this `Waitable` to the specified set (when `set` is `Some(_)`) or
    /// remove it from any set it may currently belong to (when `set` is
    /// `None`).
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

    /// Retrieve mutable access to the `WaitableCommon` for this `Waitable`.
    fn common<'a>(&self, instance: &'a mut ComponentInstance) -> Result<&'a mut WaitableCommon> {
        Ok(match self {
            Self::Host(id) => &mut instance.get_mut(*id)?.common,
            Self::Guest(id) => &mut instance.get_mut(*id)?.common,
            Self::Transmit(id) => &mut instance.get_mut(*id)?.common,
        })
    }

    /// Set or clear the pending event for this waitable and either deliver it
    /// to the first waiter, if any, or mark it as ready to be delivered to the
    /// next waiter that arrives.
    fn set_event(&self, instance: &mut ComponentInstance, event: Option<Event>) -> Result<()> {
        log::trace!("set event for {self:?}: {event:?}");
        self.common(instance)?.event = event;
        self.mark_ready(instance)
    }

    /// Take the pending event from this waitable, leaving `None` in its place.
    fn take_event(&self, instance: &mut ComponentInstance) -> Result<Option<Event>> {
        let common = self.common(instance)?;
        let event = common.event.take();
        if let Some(set) = self.common(instance)?.set {
            instance.get_mut(set)?.ready.remove(self);
        }
        Ok(event)
    }

    /// Deliver the current event for this waitable to the first waiter, if any,
    /// or else mark it as ready to be delivered to the next waiter that
    /// arrives.
    fn mark_ready(&self, instance: &mut ComponentInstance) -> Result<()> {
        if let Some(set) = self.common(instance)?.set {
            instance.get_mut(set)?.ready.insert(*self);
            if let Some((task, mode)) = instance.get_mut(set)?.waiting.pop_first() {
                let wake_on_cancel = instance.get_mut(task)?.wake_on_cancel.take();
                assert!(wake_on_cancel.is_none() || wake_on_cancel == Some(set));

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

    /// Handle the imminent delivery of the specified event, e.g. by updating
    /// the state of the stream or future.
    fn on_delivery(&self, instance: &mut ComponentInstance, event: Event) {
        match event {
            Event::FutureRead {
                pending: Some((ty, handle)),
                ..
            }
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
            Event::StreamRead {
                pending: Some((ty, handle)),
                ..
            }
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

    /// Remove this waitable from the instance's rep table.
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

/// Represents a Component Model Async `waitable-set`.
#[derive(Default)]
struct WaitableSet {
    /// Which waitables in this set have pending events, if any.
    ready: BTreeSet<Waitable>,
    /// Which guest tasks are currently waiting on this set, if any.
    waiting: BTreeMap<TableId<GuestTask>, WaitMode>,
}

impl TableDebug for WaitableSet {
    fn type_name() -> &'static str {
        "WaitableSet"
    }
}

/// Type-erased closure to lower the parameters for a guest task.
type RawLower = Box<
    dyn FnOnce(&mut dyn VMStore, &mut ComponentInstance, &mut [MaybeUninit<ValRaw>]) -> Result<()>
        + Send
        + Sync,
>;

/// Generic function pointer to lower the parameters for a guest task.
pub type LowerFn<T> = unsafe fn(
    Func,
    StoreContextMut<T>,
    &mut ComponentInstance,
    *mut u8,
    &mut [MaybeUninit<ValRaw>],
) -> Result<()>;

/// Type-erased closure to lift the result for a guest task.
type RawLift = Box<
    dyn FnOnce(
            &mut dyn VMStore,
            &mut ComponentInstance,
            &[ValRaw],
        ) -> Result<Box<dyn Any + Send + Sync>>
        + Send
        + Sync,
>;

/// Generic function pointer to lift the result for a guest task.
pub type LiftFn<T> = fn(
    Func,
    StoreContextMut<T>,
    &mut ComponentInstance,
    &[ValRaw],
) -> Result<Box<dyn Any + Send + Sync>>;

/// Type erased result of a guest task which may be downcast to the expected
/// type by a host caller (or simply ignored in the case of a guest caller; see
/// `DummyResult`).
type LiftedResult = Box<dyn Any + Send + Sync>;

/// Used to return a result from a `LiftFn` when the actual result has already
/// been lowered to a guest task's stack and linear memory.
struct DummyResult;

/// Helper struct for reseting a raw pointer to its original value on drop.
struct Reset<T: Copy>(*mut T, T);

impl<T: Copy> Drop for Reset<T> {
    fn drop(&mut self) {
        unsafe {
            *self.0 = self.1;
        }
    }
}

/// Represents the context of a `Future::poll` operation which involves resuming
/// a fiber.
///
/// See `self::poll_fn` for details.
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

/// Represents the state of a currently executing fiber which has been resumed
/// via `self::poll_fn`.
pub(crate) struct AsyncState {
    /// The `Suspend` for the current fiber (or null if no such fiber is running).
    ///
    /// See `StoreFiber` for an explanation of the signature types we use here.
    current_suspend: UnsafeCell<
        *mut Suspend<
            (Option<*mut dyn VMStore>, Result<()>),
            Option<*mut dyn VMStore>,
            (Option<*mut dyn VMStore>, Result<()>),
        >,
    >,
    /// See `PollContext`
    current_poll_cx: UnsafeCell<PollContext>,

    /// List of spawned tasks built up during a polling operation. This is
    /// drained after the poll in `poll_with_state`.
    spawned_tasks: Vec<HostTaskFuture>,
}

impl Default for AsyncState {
    fn default() -> Self {
        Self {
            current_suspend: UnsafeCell::new(ptr::null_mut()),
            current_poll_cx: UnsafeCell::new(PollContext::default()),
            spawned_tasks: Vec::new(),
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

/// Used to "stackfully" poll a future by suspending the current fiber
/// repeatedly in a loop until the future completes.
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
    /// Create a new `AsyncCx`.
    ///
    /// This will panic if called outside the scope of a `self::poll_fn` call.
    /// Consider using `Self::try_new` instead to avoid panicking.
    pub(crate) fn new(store: &mut StoreOpaque) -> Self {
        Self::try_new(store).unwrap()
    }

    /// Create a new `AsyncCx`.
    ///
    /// This will return `None` if called outside the scope of a `self::poll_fn`
    /// call.
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

    /// Poll the specified future using `Self::current_poll_cx`.
    ///
    /// This will panic if called recursively using the same `AsyncState`.
    ///
    /// SAFETY: TODO
    unsafe fn poll<U>(&self, mut future: Pin<&mut (dyn Future<Output = U> + Send)>) -> Poll<U> {
        let poll_cx = *self.current_poll_cx;
        let _reset = Reset(self.current_poll_cx, poll_cx);
        *self.current_poll_cx = PollContext::default();
        assert!(!poll_cx.future_context.is_null());
        future.as_mut().poll(&mut *poll_cx.future_context)
    }

    /// "Stackfully" poll the specified future by alternately polling it and
    /// suspending the current fiber until `Future::poll` returns `Ready`.
    ///
    /// SAFETY: TODO
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

    /// Suspend the current fiber, optionally transfering exclusive access to
    /// the store back to the code which resumed it.
    ///
    /// SAFETY: TODO
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

/// Represents the Component Model Async state of a (sub-)component instance.
#[derive(Default)]
struct InstanceState {
    /// Whether backpressure is set for this instance
    backpressure: bool,
    /// Whether this instance can be entered
    do_not_enter: bool,
    /// Pending calls for this instance which require `Self::backpressure` to be
    /// `true` and/or `Self::do_not_enter` to be false before they can proceed.
    pending: BTreeMap<TableId<GuestTask>, GuestCallKind>,
}

/// Represents the Component Model Async state of a top-level component instance
/// (i.e. a `super::ComponentInstance`).
pub struct ConcurrentState {
    /// The currently running guest task, if any.
    guest_task: Option<TableId<GuestTask>>,
    /// The set of pending host and background tasks, if any.
    ///
    /// We must wrap this in a `Mutex` to ensure that `ComponentInstance` and
    /// `Store` satisfy a `Sync` bound, but it can't actually be accessed from
    /// more than one thread at a time.
    ///
    /// See `ComponentInstance::poll_until` for where we temporarily take this
    /// out, poll it, then put it back to avoid any mutable aliasing hazards.
    futures: Mutex<Option<FuturesUnordered<HostTaskFuture>>>,
    /// The table of waitables, waitable sets, etc.
    table: Table,
    /// Per (sub-)component instance states.
    ///
    /// See `InstanceState` for details and note that this map is lazily
    /// populated as needed.
    // TODO: this can and should be a `PrimaryMap`
    instance_states: HashMap<RuntimeComponentInstanceIndex, InstanceState>,
    /// Tables for tracking per-(sub-)component waitable handles and their
    /// states.
    waitable_tables: PrimaryMap<RuntimeComponentInstanceIndex, StateTable<WaitableState>>,
    /// The "high priority" work queue for this instance's event loop.
    high_priority: Vec<WorkItem>,
    /// The "high priority" work queue for this instance's event loop.
    low_priority: Vec<WorkItem>,
    /// A place to stash the reason a fiber is suspending so that the code which
    /// resumed it will know under what conditions the fiber should be resumed
    /// again.
    suspend_reason: Option<SuspendReason>,
    /// A cached fiber which is waiting for work to do.
    ///
    /// This helps us avoid creating a new fiber for each `GuestCall` work item.
    worker: Option<StoreFiber<'static>>,
    /// A place to stash the work item for which we're resuming a worker fiber.
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

    /// Drop any and all fibers prior to dropping the `ComponentInstance` and
    /// `Store` to which `self` belongs.
    ///
    /// This is necessary to avoid possible use-after-free bugs due to fibers
    /// which may still have access to the `Store` and/or `ComponentInstance`.
    pub(crate) fn drop_fibers(&mut self) {
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

/// Provide a type hint to compiler about the shape of a parameter lower
/// closure.
fn for_any_lower<
    F: FnOnce(&mut dyn VMStore, &mut ComponentInstance, &mut [MaybeUninit<ValRaw>]) -> Result<()>
        + Send
        + Sync,
>(
    fun: F,
) -> F {
    fun
}

/// Provide a type hint to compiler about the shape of a result lift closure.
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

/// Wrap the specified future in a `poll_fn` which asserts that the future is
/// only polled from the event loop of the specified `ComponentInstance`.
///
/// See `Instance::run` for details.
fn checked<F: Future + Send + 'static>(
    instance: Instance,
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
                InstanceThreadLocalState::Detached { handle, .. }
                | InstanceThreadLocalState::Attached { handle } => handle.0 == instance.0,
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

/// Assert that `Instance::run[_with]` has not been called from within an
/// instance's event loop.
fn check_recursive_run() {
    INSTANCE_STATE.with(|v| {
        if !matches!(v.get(), InstanceThreadLocalState::None) {
            panic!("Recursive `Instance::run[_with]` calls not supported")
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
    /// The raw `wasmtime_fiber::Fiber`.
    ///
    /// Note that the signatures for resuming, yielding, and returning all allow
    /// a `*mut dyn VMStore` to be passed to and from the fiber.  This can be
    /// used by the fiber to indicate whether it needs exclusive access to the
    /// store across suspend points (in which case it will pass `None` when
    /// suspending or yielding, meaning the store must not be used at all until
    /// the fiber is resumed again) or whether it is giving up exclusive access
    /// (in which case it will pass `Some(_)` when suspending or yielding,
    /// meaning exclusive access may be given to another fiber that runs
    /// concurrently.
    fiber: Option<
        Fiber<
            'a,
            (Option<*mut dyn VMStore>, Result<()>),
            Option<*mut dyn VMStore>,
            (Option<*mut dyn VMStore>, Result<()>),
        >,
    >,
    /// See `AsyncWasmCallState`
    state: Option<AsyncWasmCallState>,
    /// The Wasmtime `Engine` to which this fiber belongs.
    engine: Engine,
    /// The current `Suspend` for this fiber (or null if it's not currently
    /// running).
    suspend: *mut *mut Suspend<
        (Option<*mut dyn VMStore>, Result<()>),
        Option<*mut dyn VMStore>,
        (Option<*mut dyn VMStore>, Result<()>),
    >,
    /// The stack limit used for handling traps from guest code.
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
            // SAFETY: `self` is a valid pointer and we pass a `store` parameter
            // of `None` to inform the resumed fiber that it does _not_ have
            // access to the store.
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

/// Create a new `StoreFiber` which runs the specified closure.
///
/// SAFETY: `store` must be a valid pointer to a store, and the caller must
/// confer exclusive access to that store until this function returns.
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

/// Resume the specified fiber, optionally granting it exclusive access to the
/// specified store.
///
/// This will either return the store or `None` depending on whether the fiber
/// needs to retain exclusive access to the store accross suspend points.  See
/// `StoreFiber::fiber` for details.
///
/// SAFETY: If a store pointer is provided, the caller must confer exclusive
/// access to the fiber until it is either dropped, resolved, or forgotten, or
/// until it gives back the store when suspending or yielding.
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

/// See `resume_fiber_raw`
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

/// Suspend the current fiber, optionally returning exclusive access to the
/// specified store to the code which resumed the fiber.
///
/// SAFETY: `suspend` must be a valid pointer.  Additionally, if a store pointer
/// is provided, the fiber must give up access to the store until it is given
/// back access when next resumed.
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

/// Helper struct for packaging parameters to be passed to
/// `ComponentInstance::waitable_check` for calls to `waitable-set.wait` or
/// `waitable-set.poll`.
pub(crate) struct WaitableCheckParams {
    set: TableId<WaitableSet>,
    memory: *mut VMMemoryDefinition,
    payload: u32,
    caller_instance: RuntimeComponentInstanceIndex,
}

/// Helper enum for passing parameters to `ComponentInstance::waitable_check`.
pub(crate) enum WaitableCheck {
    Wait(WaitableCheckParams),
    Poll(WaitableCheckParams),
    Yield,
}

/// Helper struct for nulling out an `AtomicPtr<u8>` on drop.
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

/// Represents a guest task called from the host, prepared using `prepare_call`.
pub(crate) struct PreparedCall<R> {
    /// The guest export to be called
    handle: Func,
    /// The guest task created by `prepare_call`
    task: TableId<GuestTask>,
    /// The number of lowered core Wasm parameters to pass to the call.
    param_count: usize,
    /// The `oneshot::Receiver` to which the result of the call will be
    /// delivered when it is available.
    rx: oneshot::Receiver<LiftedResult>,
    /// A type-erased pointer to the parameters to pass to the call.
    ///
    /// See `prepare_call` for details on how this is used.
    params: Arc<AtomicPtr<u8>>,
    _phantom: PhantomData<R>,
}

impl<R> PreparedCall<R> {
    /// Retrieve the parameter pointer for this call.
    ///
    /// See `prepare_call` for details on how this is used.
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
///
/// In addition, the pointers in the `FuncData` for `handle` must be valid for
/// as long as the guest task we create here exists.
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
                        // SAFETY: Per the contract of `prepare_call`, `ptr`
                        // must be valid and `lower_params` must either use it
                        // safely or not at all.
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
                        // SAFETY: Per the contract of `prepare_call`, the
                        // callback will remain valid at least as long is this
                        // task exists.
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

/// Queue a call previously prepared using `prepare_call` to be run as part of
/// the associated `ComponentInstance`'s event loop.
///
/// The returned future will resolve to the result once it is available, but
/// must only be polled via the instance's event loop.  See `Instance::run` for
/// details.
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

    queue_call0(store.as_context_mut(), handle, task, param_count)?;

    Ok(checked(
        store[handle.0].instance,
        rx.map(|result| {
            result
                .map(|v| *v.downcast().unwrap())
                .map_err(anyhow::Error::from)
        }),
    ))
}

/// Queue a call previously prepared using `prepare_call` to be run as part of
/// the associated `ComponentInstance`'s event loop.
fn queue_call0<T: 'static>(
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
        log::trace!("queueing call {guest_task:?}");

        // SAFETY: `callee`, `callback`, and `post_return` are valid pointers
        // (with signatures appropriate for this call) and will remain valid as
        // long as this instance is valid.
        unsafe {
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
        }

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
