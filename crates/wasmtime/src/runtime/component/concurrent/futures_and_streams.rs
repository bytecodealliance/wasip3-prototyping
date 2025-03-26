use {
    super::{
        table::TableId, Event, GlobalErrorContextRefCount, HostTaskFuture, HostTaskOutput,
        HostTaskResult, LocalErrorContextRefCount, Promise, StateTable, WaitableCommon,
        WaitableState,
    },
    crate::{
        component::{
            func::{self, Lift, LiftContext, LowerContext, Options},
            matching::InstanceType,
            values::{ErrorContextAny, FutureAny, StreamAny},
            Instance, Lower, Val, WasmList, WasmStr,
        },
        store::StoreId,
        vm::{component::ComponentInstance, SendSyncPtr, VMFuncRef, VMMemoryDefinition},
        AsContextMut, StoreContextMut, ValRaw,
    },
    anyhow::{anyhow, bail, ensure, Context, Result},
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        future::{self, FutureExt},
        stream::StreamExt,
    },
    std::{
        any::Any,
        boxed::Box,
        future::Future,
        iter,
        marker::PhantomData,
        mem::{self, MaybeUninit},
        ops::{Deref, DerefMut},
        ptr::NonNull,
        string::{String, ToString},
        sync::{Arc, Mutex},
        task::{Poll, Waker},
        vec::Vec,
    },
    wasmtime_environ::component::{
        CanonicalAbiInfo, ComponentTypes, InterfaceType, RuntimeComponentInstanceIndex,
        StringEncoding, TypeComponentGlobalErrorContextTableIndex,
        TypeComponentLocalErrorContextTableIndex, TypeFutureTableIndex, TypeStreamTableIndex,
    },
};

const BLOCKED: usize = 0xffff_ffff;
const CLOSED: usize = 0x8000_0000;

#[derive(Copy, Clone, Debug)]
pub(super) enum TableIndex {
    Stream(TypeStreamTableIndex),
    Future(TypeFutureTableIndex),
}

/// Action to take after writing
enum PostWrite {
    /// Continue performing writes
    Continue,
    /// Close the channel post-write, possibly with an error
    Close(Option<TypeComponentGlobalErrorContextTableIndex>),
}

pub(crate) enum HostReadResult<B> {
    /// Values sent to the stream
    Values(B),
    /// When host streams end, they may have an attached error-context
    #[allow(unused)]
    EndOfStream(Option<ErrorContext>),
}

fn payload(ty: TableIndex, types: &Arc<ComponentTypes>) -> Option<InterfaceType> {
    match ty {
        TableIndex::Future(ty) => types[types[ty].ty].payload,
        TableIndex::Stream(ty) => types[types[ty].ty].payload,
    }
}

fn get_mut_by_index_from(
    state_table: &mut StateTable<WaitableState>,
    ty: TableIndex,
    index: u32,
) -> Result<(u32, &mut StreamFutureState)> {
    Ok(match ty {
        TableIndex::Stream(ty) => {
            let (rep, WaitableState::Stream(actual_ty, state)) =
                state_table.get_mut_by_index(index)?
            else {
                bail!("invalid stream handle");
            };
            if *actual_ty != ty {
                bail!("invalid stream handle");
            }
            (rep, state)
        }
        TableIndex::Future(ty) => {
            let (rep, WaitableState::Future(actual_ty, state)) =
                state_table.get_mut_by_index(index)?
            else {
                bail!("invalid future handle");
            };
            if *actual_ty != ty {
                bail!("invalid future handle");
            }
            (rep, state)
        }
    })
}

fn waitable_state(ty: TableIndex, state: StreamFutureState) -> WaitableState {
    match ty {
        TableIndex::Stream(ty) => WaitableState::Stream(ty, state),
        TableIndex::Future(ty) => WaitableState::Future(ty, state),
    }
}

fn accept<T: func::Lower + Send + Sync + 'static, B: Buffer<T>, U>(
    values: B,
    mut offset: usize,
    transmit_id: TableId<TransmitState>,
    tx: oneshot::Sender<()>,
) -> impl FnOnce(&mut ComponentInstance, Reader) -> Result<usize> + Send + Sync + 'static {
    move |instance, reader| {
        let count = match reader {
            Reader::Guest {
                lower: RawLowerContext { options },
                ty,
                address,
                count,
            } => {
                let mut store = unsafe { StoreContextMut::<U>(&mut *instance.store().cast()) };
                let ptr = instance as *mut _;
                let types = instance.component_types();
                let lower =
                    unsafe { &mut LowerContext::new(store.as_context_mut(), options, types, ptr) };
                if address % usize::try_from(T::ALIGN32)? != 0 {
                    bail!("read pointer not aligned");
                }
                lower
                    .as_slice_mut()
                    .get_mut(address..)
                    .and_then(|b| b.get_mut(..T::SIZE32 * count))
                    .ok_or_else(|| anyhow::anyhow!("read pointer out of bounds of memory"))?;

                let count = values.len().min(usize::try_from(count).unwrap());

                if let Some(ty) = payload(ty, types) {
                    T::store_list(lower, ty, address, &values[offset..][..count])?;
                }
                offset += count;

                if offset < values.len() {
                    let transmit = instance.get_mut(transmit_id)?;
                    assert!(matches!(&transmit.write, WriteState::Open));

                    transmit.write = WriteState::HostReady {
                        accept: Box::new(accept::<T, B, U>(values, offset, transmit_id, tx)),
                        post_write: PostWrite::Continue,
                    };
                } else {
                    _ = tx.send(());
                }

                count
            }
            Reader::Host { accept } => {
                assert!(offset == 0); // todo: do we need to handle offset != 0?
                let count = values.len();
                accept(Box::new(values))?;
                _ = tx.send(());
                count
            }
        };

        Ok(count)
    }
}

/// Represents the state of a stream or future handle.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum StreamFutureState {
    /// Both the read and write ends are owned by the same component instance.
    Local,
    /// Only the write end is owned by this component instance.
    Write,
    /// Only the read end is owned by this component instance.
    Read,
    /// A read or write is in progress.
    Busy,
}

/// Represents the state associated with an error context
#[derive(Debug, PartialEq, Eq, PartialOrd)]
pub(super) struct ErrorContextState {
    /// Debug message associated with the error context
    pub(crate) debug_msg: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FlatAbi {
    pub(super) size: u32,
    pub(super) align: u32,
}

enum StreamOrFutureEvent<B> {
    Write {
        values: B,
        post_write: PostWrite,
        tx: oneshot::Sender<()>,
    },
    CloseWriter {
        err_ctx: Option<ErrorContext>,
    },
    WatchWriter {
        tx: oneshot::Sender<()>,
    },
    Read {
        tx: oneshot::Sender<HostReadResult<B>>,
    },
    CloseReader,
    WatchReader {
        tx: oneshot::Sender<()>,
    },
}

/// Send the specified value to the specified `Sender`.
///
/// This will panic if there is no room in the channel's buffer, so it should
/// only be used in a context where there is at least one empty spot in the
/// buffer.  It will silently ignore any other error (e.g. if the `Receiver` has
/// been dropped).
fn send<T>(tx: &mut mpsc::Sender<T>, value: T) {
    if let Err(e) = tx.try_send(value) {
        if e.is_full() {
            unreachable!();
        }
    }
}

/// Trivial container type for sending exactly one element at a time to a
/// `StringWriter`.
pub struct Single<T>(pub T);

impl<T> Deref for Single<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        // TODO: Is there a safe way to do this?
        unsafe { std::slice::from_raw_parts(&self.0, 1) }
    }
}

impl<T> From<Vec<T>> for Single<T> {
    fn from(vec: Vec<T>) -> Self {
        assert_eq!(vec.len(), 1);
        Self(vec.into_iter().next().unwrap())
    }
}

pub trait Buffer<T>: Deref<Target = [T]> + From<Vec<T>> + Send + Sync + 'static {}

impl<T: Send + Sync + 'static> Buffer<T> for Vec<T> {}

impl Buffer<u8> for Bytes {}

impl<T: Send + Sync + 'static> Buffer<T> for Single<T> {}

pub trait Ready {
    fn ready(&mut self) -> impl Future<Output = ()> + Send;
}

struct WatchInner<T> {
    inner: T,
    rx: oneshot::Receiver<()>,
    waker: Option<Waker>,
}

/// Wrapper struct which may be converted to the inner value as needed.
///
/// This object is normally paired with a `Promise<()>` which represents a state
/// change on the inner value, resolving when that state change happens _or_
/// when the `Watch` is converted back into the inner value -- whichever happens
/// first.
pub struct Watch<T>(Arc<Mutex<Option<WatchInner<T>>>>);

impl<T: Ready> Watch<T> {
    /// Convert this object into its inner value.
    ///
    /// Calling this function will cause the associated `Promise<()>` to resolve
    /// immediately if it hasn't already.
    pub fn into_inner(self) -> T {
        let inner = self.0.try_lock().unwrap().take().unwrap();
        if let Some(waker) = inner.waker {
            waker.wake();
        }
        inner.inner
    }
}

fn watch<T: Send + 'static>(
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    rx: oneshot::Receiver<()>,
    inner: T,
) -> (Promise<()>, Watch<T>) {
    let inner = Arc::new(Mutex::new(Some(WatchInner {
        inner,
        rx,
        waker: None,
    })));
    (
        Promise {
            inner: Box::pin(future::poll_fn({
                let inner = inner.clone();

                move |cx| {
                    if let Some(inner) = inner.try_lock().unwrap().deref_mut() {
                        match inner.rx.poll_unpin(cx) {
                            Poll::Ready(_) => Poll::Ready(()),
                            Poll::Pending => {
                                inner.waker = Some(cx.waker().clone());
                                Poll::Pending
                            }
                        }
                    } else {
                        Poll::Ready(())
                    }
                }
            })),
            instance,
            id,
        },
        Watch(inner),
    )
}

/// Represents the writable end of a Component Model `future`.
pub struct FutureWriter<T> {
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    tx: Option<mpsc::Sender<StreamOrFutureEvent<Single<T>>>>,
}

impl<T> FutureWriter<T> {
    fn new(
        tx: Option<mpsc::Sender<StreamOrFutureEvent<Single<T>>>>,
        instance: &mut ComponentInstance,
    ) -> Self {
        Self {
            instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
            id: unsafe { (*instance.store()).store_opaque().id() },
            tx,
        }
    }

    /// Write the specified value to this `future`.
    ///
    /// The returned `Promise` will yield `true` if the read end accepted the
    /// value; otherwise it will return `false`, meaning the read end was closed
    /// before the value could be delivered.
    pub fn write(mut self, value: T) -> Promise<bool> {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.take().unwrap(),
            StreamOrFutureEvent::Write {
                values: Single(value),
                post_write: PostWrite::Close(None),
                tx,
            },
        );
        Promise {
            inner: Box::pin(rx.map(|v| v.is_ok())),
            instance: self.instance,
            id: self.id,
        }
    }

    /// Close this object with and error instead of writing a value.
    ///
    /// # Arguments
    ///
    /// * `err_ctx` - The handle of an error context that should be reported with the stream closure
    ///
    pub fn close_with_error(mut self, err_ctx: ErrorContext) {
        send(
            &mut self.tx.take().unwrap(),
            StreamOrFutureEvent::CloseWriter {
                err_ctx: Some(err_ctx),
            },
        );
    }

    /// Convert this object into a `Promise` which will resolve when the read
    /// end of this `future` is closed, plus a `Watch` which can be used to
    /// retrieve the `FutureWriter` again.
    ///
    /// Note that calling `Watch::into_inner` on the returned `Watch` will have
    /// the side effect of causing the `Promise` to resolve immediately if it
    /// hasn't already.
    pub fn watch_reader(mut self) -> (Promise<()>, Watch<Self>)
    where
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchReader { tx },
        );
        let instance = self.instance;
        let id = self.id;
        watch(instance, id, rx, self)
    }
}

impl<T: Send> Ready for FutureWriter<T> {
    async fn ready(&mut self) {
        _ = future::poll_fn(|cx| self.tx.as_mut().unwrap().poll_ready(cx)).await;
    }
}

impl<T> Drop for FutureWriter<T> {
    fn drop(&mut self) {
        if let Some(mut tx) = self.tx.take() {
            send(&mut tx, StreamOrFutureEvent::CloseWriter { err_ctx: None });
        }
    }
}

/// Represents the readable end of a Component Model `future`.
///
/// In order to actually read from or close this `future`, first convert it to a
/// [`FutureReader`] using the `into_reader` method.
///
/// Note that if a value of this type is dropped without either being converted
/// to a `FutureReader` or passed to the guest, any writes on the write end may
/// block forever.
pub struct HostFuture<T> {
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    rep: u32,
    _phantom: PhantomData<T>,
}

impl<T> HostFuture<T> {
    fn new(rep: u32, instance: &mut ComponentInstance) -> Self {
        Self {
            instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
            id: unsafe { (*instance.store()).store_opaque().id() },
            rep,
            _phantom: PhantomData,
        }
    }

    /// Convert this object into a [`FutureReader`].
    pub fn into_reader<U, S: AsContextMut<Data = U>>(self, mut store: S) -> FutureReader<T>
    where
        T: func::Lower + func::Lift + Send + Sync + 'static,
    {
        let store = store.as_context_mut();
        assert_eq!(store.0.id(), self.id);
        let instance = unsafe { &mut *self.instance.as_ptr() };
        FutureReader {
            instance: self.instance,
            id: self.id,
            rep: self.rep,
            tx: Some(instance.start_event_loop::<_, _, U>(self.rep)),
        }
    }

    /// Convert this `FutureReader` into a [`Val`].
    // See TODO comment for `FutureAny`; this is prone to handle leakage.
    pub fn into_val(self) -> Val {
        Val::Future(FutureAny(self.rep))
    }

    /// Attempt to convert the specified [`Val`] to a `FutureReader`.
    pub fn from_val<U, S: AsContextMut<Data = U>>(
        mut store: S,
        instance: Instance,
        value: &Val,
    ) -> Result<Self> {
        let Val::Future(FutureAny(rep)) = value else {
            bail!("expected `future`; got `{}`", value.desc());
        };
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[instance.0].as_ref().unwrap().instance_ptr() };
        instance.get(TableId::<TransmitHandle>::new(*rep))?; // Just make sure it's present
        Ok(Self::new(*rep, instance))
    }

    fn lift_from_index(cx: &mut LiftContext<'_>, ty: InterfaceType, index: u32) -> Result<Self> {
        match ty {
            InterfaceType::Future(src) => {
                let instance = unsafe { &mut *cx.instance };
                let state_table = instance.state_table(TableIndex::Future(src));
                let (rep, state) =
                    get_mut_by_index_from(state_table, TableIndex::Future(src), index)?;

                match state {
                    StreamFutureState::Local => {
                        *state = StreamFutureState::Write;
                    }
                    StreamFutureState::Read => {
                        state_table.remove_by_index(index)?;
                    }
                    StreamFutureState::Write => bail!("cannot transfer write end of future"),
                    StreamFutureState::Busy => bail!("cannot transfer busy future"),
                }

                Ok(Self::new(rep, instance))
            }
            _ => func::bad_type_info(),
        }
    }
}

pub(crate) fn lower_future_to_index<U>(
    rep: u32,
    cx: &mut LowerContext<'_, U>,
    ty: InterfaceType,
) -> Result<u32> {
    match ty {
        InterfaceType::Future(dst) => {
            let instance = unsafe { &mut *cx.instance };
            let state = instance.get(TableId::<TransmitHandle>::new(rep))?.state;
            let rep = instance.get(state)?.read_handle.rep();

            instance
                .state_table(TableIndex::Future(dst))
                .insert(rep, WaitableState::Future(dst, StreamFutureState::Read))
        }
        _ => func::bad_type_info(),
    }
}

unsafe impl<T> func::ComponentType for HostFuture<T> {
    const ABI: CanonicalAbiInfo = CanonicalAbiInfo::SCALAR4;

    type Lower = <u32 as func::ComponentType>::Lower;

    fn typecheck(ty: &InterfaceType, _types: &InstanceType<'_>) -> Result<()> {
        match ty {
            InterfaceType::Future(_) => Ok(()),
            other => bail!("expected `future`, found `{}`", func::desc(other)),
        }
    }
}

unsafe impl<T> func::Lower for HostFuture<T> {
    fn lower<U>(
        &self,
        cx: &mut LowerContext<'_, U>,
        ty: InterfaceType,
        dst: &mut MaybeUninit<Self::Lower>,
    ) -> Result<()> {
        lower_future_to_index(self.rep, cx, ty)?.lower(cx, InterfaceType::U32, dst)
    }

    fn store<U>(
        &self,
        cx: &mut LowerContext<'_, U>,
        ty: InterfaceType,
        offset: usize,
    ) -> Result<()> {
        lower_future_to_index(self.rep, cx, ty)?.store(cx, InterfaceType::U32, offset)
    }
}

unsafe impl<T> func::Lift for HostFuture<T> {
    fn lift(cx: &mut LiftContext<'_>, ty: InterfaceType, src: &Self::Lower) -> Result<Self> {
        let index = u32::lift(cx, InterfaceType::U32, src)?;
        Self::lift_from_index(cx, ty, index)
    }

    fn load(cx: &mut LiftContext<'_>, ty: InterfaceType, bytes: &[u8]) -> Result<Self> {
        let index = u32::load(cx, InterfaceType::U32, bytes)?;
        Self::lift_from_index(cx, ty, index)
    }
}

impl<T> From<FutureReader<T>> for HostFuture<T> {
    fn from(mut value: FutureReader<T>) -> Self {
        value.tx.take();

        Self {
            instance: value.instance,
            id: value.id,
            rep: value.rep,
            _phantom: PhantomData,
        }
    }
}

/// Represents the readable end of a Component Model `future`.
///
/// In order to pass this end to guest code, first convert it to a
/// [`HostFuture`] using the `into` method.
pub struct FutureReader<T> {
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    rep: u32,
    tx: Option<mpsc::Sender<StreamOrFutureEvent<Single<T>>>>,
}

impl<T> FutureReader<T> {
    fn new(
        rep: u32,
        tx: Option<mpsc::Sender<StreamOrFutureEvent<Single<T>>>>,
        instance: &mut ComponentInstance,
    ) -> Self {
        Self {
            instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
            id: unsafe { (*instance.store()).store_opaque().id() },
            rep,
            tx,
        }
    }

    /// Read the value from this `future`.
    ///
    /// The returned `Promise` will yield `Err(None)` if the guest has trapped
    /// before it could produce a result.
    pub fn read(mut self) -> Promise<Result<T, Option<ErrorContext>>>
    where
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.take().unwrap(),
            StreamOrFutureEvent::Read { tx },
        );
        Promise {
            inner: Box::pin(rx.map(|v| match v {
                Ok(HostReadResult::Values(v)) => Ok(v.0),
                Ok(HostReadResult::EndOfStream(None)) => unreachable!(),
                Err(_) => Err(None),
                Ok(HostReadResult::EndOfStream(s)) => Err(s),
            })),
            instance: self.instance,
            id: self.id,
        }
    }

    /// Convert this object into a `Promise` which will resolve when the write
    /// end of this `future` is closed, plus a `Watch` which can be used to
    /// retrieve the `FutureReader` again.
    ///
    /// Note that calling `Watch::into_inner` on the returned `Watch` will have
    /// the side effect of causing the `Promise` to resolve immediately if it
    /// hasn't already.
    pub fn watch_writer(mut self) -> (Promise<()>, Watch<Self>)
    where
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchWriter { tx },
        );
        let instance = self.instance;
        let id = self.id;
        watch(instance, id, rx, self)
    }
}

impl<T: Send> Ready for FutureReader<T> {
    async fn ready(&mut self) {
        _ = future::poll_fn(|cx| self.tx.as_mut().unwrap().poll_ready(cx)).await;
    }
}

impl<T> Drop for FutureReader<T> {
    fn drop(&mut self) {
        if let Some(mut tx) = self.tx.take() {
            send(&mut tx, StreamOrFutureEvent::CloseReader);
        }
    }
}

/// Represents the writable end of a Component Model `stream`.
pub struct StreamWriter<B> {
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    tx: Option<mpsc::Sender<StreamOrFutureEvent<B>>>,
}

impl<B> StreamWriter<B> {
    fn new(
        tx: Option<mpsc::Sender<StreamOrFutureEvent<B>>>,
        instance: &mut ComponentInstance,
    ) -> Self {
        Self {
            instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
            id: unsafe { (*instance.store()).store_opaque().id() },
            tx,
        }
    }

    /// Write the specified values to the `stream`.
    ///
    /// The returned `Promise` will yield `None` if the read end has been closed
    /// before it could accept all of the values; otherwise it will yield
    /// `self`.
    pub fn write(mut self, values: B) -> Promise<Option<StreamWriter<B>>>
    where
        B: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(
            self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::Write {
                values,
                post_write: PostWrite::Continue,
                tx,
            },
        );
        let instance = self.instance;
        let id = self.id;
        Promise {
            inner: Box::pin(rx.map(|v| match v {
                Ok(()) => Some(self),
                Err(_) => {
                    self.tx.take();
                    None
                }
            })),
            instance,
            id,
        }
    }

    /// Close this object with a final error.
    ///
    /// # Arguments
    ///
    /// * `err_ctx` - The handle of an error context that should be reported with the stream closure.
    ///
    pub fn close_with_error(mut self, err_ctx: ErrorContext) {
        send(
            &mut self.tx.take().unwrap(),
            StreamOrFutureEvent::CloseWriter {
                err_ctx: Some(err_ctx),
            },
        );
    }

    /// Convert this object into a `Promise` which will resolve when the read
    /// end of this `stream` is closed, plus a `Watch` which can be used to
    /// retrieve the `StreamWriter` again.
    ///
    /// Note that calling `Watch::into_inner` on the returned `Watch` will have
    /// the side effect of causing the `Promise` to resolve immediately if it
    /// hasn't already.
    pub fn watch_reader(mut self) -> (Promise<()>, Watch<Self>)
    where
        B: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchReader { tx },
        );
        let instance = self.instance;
        let id = self.id;
        watch(instance, id, rx, self)
    }
}

impl<T: Send> Ready for StreamWriter<T> {
    async fn ready(&mut self) {
        _ = future::poll_fn(|cx| self.tx.as_mut().unwrap().poll_ready(cx)).await;
    }
}

impl<T> Drop for StreamWriter<T> {
    fn drop(&mut self) {
        if let Some(mut tx) = self.tx.take() {
            send(&mut tx, StreamOrFutureEvent::CloseWriter { err_ctx: None });
        }
    }
}

/// Represents the readable end of a Component Model `stream`.
///
/// In order to actually read from or close this `stream`, first convert it to a
/// [`FutureReader`] using the `into_reader` method.
///
/// Note that if a value of this type is dropped without either being converted
/// to a `StreamReader` or passed to the guest, any writes on the write end may
/// block forever.
pub struct HostStream<T> {
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    rep: u32,
    _phantom: PhantomData<T>,
}

impl<T> HostStream<T> {
    fn new(rep: u32, instance: *mut ComponentInstance) -> Self {
        Self {
            instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
            id: unsafe { (*(*instance).store()).store_opaque().id() },
            rep,
            _phantom: PhantomData,
        }
    }

    /// Convert this object into a [`StreamReader`].
    pub fn into_reader<B, U, S: AsContextMut<Data = U>>(self, mut store: S) -> StreamReader<B>
    where
        T: func::Lower + func::Lift + Sync + Send + 'static,
        B: Buffer<T>,
    {
        let store = store.as_context_mut();
        assert_eq!(store.0.id(), self.id);
        let instance = unsafe { &mut *self.instance.as_ptr() };
        StreamReader {
            instance: self.instance,
            id: self.id,
            rep: self.rep,
            tx: Some(instance.start_event_loop::<_, _, U>(self.rep)),
        }
    }

    /// Convert this `HostStream` into a [`Val`].
    // See TODO comment for `StreamAny`; this is prone to handle leakage.
    pub fn into_val(self) -> Val {
        Val::Stream(StreamAny(self.rep))
    }

    /// Attempt to convert the specified [`Val`] to a `HostStream`.
    pub fn from_val<U, S: AsContextMut<Data = U>>(
        mut store: S,
        instance: Instance,
        value: &Val,
    ) -> Result<Self> {
        let Val::Stream(StreamAny(rep)) = value else {
            bail!("expected `stream`; got `{}`", value.desc());
        };
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[instance.0].as_ref().unwrap().instance_ptr() };
        instance.get(TableId::<TransmitHandle>::new(*rep))?;
        Ok(Self::new(*rep, instance))
    }

    fn lift_from_index(cx: &mut LiftContext<'_>, ty: InterfaceType, index: u32) -> Result<Self> {
        match ty {
            InterfaceType::Stream(src) => {
                let instance = unsafe { &mut *cx.instance };
                let state_table = instance.state_table(TableIndex::Stream(src));
                let (rep, state) =
                    get_mut_by_index_from(state_table, TableIndex::Stream(src), index)?;

                match state {
                    StreamFutureState::Local => {
                        *state = StreamFutureState::Write;
                    }
                    StreamFutureState::Read => {
                        state_table.remove_by_index(index)?;
                    }
                    StreamFutureState::Write => bail!("cannot transfer write end of stream"),
                    StreamFutureState::Busy => bail!("cannot transfer busy stream"),
                }

                Ok(Self::new(rep, instance))
            }
            _ => func::bad_type_info(),
        }
    }
}

pub(crate) fn lower_stream_to_index<U>(
    rep: u32,
    cx: &mut LowerContext<'_, U>,
    ty: InterfaceType,
) -> Result<u32> {
    match ty {
        InterfaceType::Stream(dst) => {
            let instance = unsafe { &mut *cx.instance };
            let state = instance.get(TableId::<TransmitHandle>::new(rep))?.state;
            let rep = instance.get(state)?.read_handle.rep();

            instance
                .state_table(TableIndex::Stream(dst))
                .insert(rep, WaitableState::Stream(dst, StreamFutureState::Read))
        }
        _ => func::bad_type_info(),
    }
}

unsafe impl<T> func::ComponentType for HostStream<T> {
    const ABI: CanonicalAbiInfo = CanonicalAbiInfo::SCALAR4;

    type Lower = <u32 as func::ComponentType>::Lower;

    fn typecheck(ty: &InterfaceType, _types: &InstanceType<'_>) -> Result<()> {
        match ty {
            InterfaceType::Stream(_) => Ok(()),
            other => bail!("expected `stream`, found `{}`", func::desc(other)),
        }
    }
}

unsafe impl<T> func::Lower for HostStream<T> {
    fn lower<U>(
        &self,
        cx: &mut LowerContext<'_, U>,
        ty: InterfaceType,
        dst: &mut MaybeUninit<Self::Lower>,
    ) -> Result<()> {
        lower_stream_to_index(self.rep, cx, ty)?.lower(cx, InterfaceType::U32, dst)
    }

    fn store<U>(
        &self,
        cx: &mut LowerContext<'_, U>,
        ty: InterfaceType,
        offset: usize,
    ) -> Result<()> {
        lower_stream_to_index(self.rep, cx, ty)?.store(cx, InterfaceType::U32, offset)
    }
}

unsafe impl<T> func::Lift for HostStream<T> {
    fn lift(cx: &mut LiftContext<'_>, ty: InterfaceType, src: &Self::Lower) -> Result<Self> {
        let index = u32::lift(cx, InterfaceType::U32, src)?;
        Self::lift_from_index(cx, ty, index)
    }

    fn load(cx: &mut LiftContext<'_>, ty: InterfaceType, bytes: &[u8]) -> Result<Self> {
        let index = u32::load(cx, InterfaceType::U32, bytes)?;
        Self::lift_from_index(cx, ty, index)
    }
}

impl<T, B: Buffer<T>> From<StreamReader<B>> for HostStream<T> {
    fn from(mut value: StreamReader<B>) -> Self {
        value.tx.take();

        Self {
            instance: value.instance,
            id: value.id,
            rep: value.rep,
            _phantom: PhantomData,
        }
    }
}

/// Represents the readable end of a Component Model `stream`.
///
/// In order to pass this end to guest code, first convert it to a
/// [`HostStream`] using the `into` method.
pub struct StreamReader<B> {
    instance: SendSyncPtr<ComponentInstance>,
    id: StoreId,
    rep: u32,
    tx: Option<mpsc::Sender<StreamOrFutureEvent<B>>>,
}

impl<B> StreamReader<B> {
    fn new(
        rep: u32,
        tx: Option<mpsc::Sender<StreamOrFutureEvent<B>>>,
        instance: &mut ComponentInstance,
    ) -> Self {
        Self {
            instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
            id: unsafe { (*instance.store()).store_opaque().id() },
            rep,
            tx,
        }
    }

    /// Read the value from this `stream`.
    pub fn read(mut self) -> Promise<Result<(StreamReader<B>, B), Option<ErrorContext>>>
    where
        B: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(self.tx.as_mut().unwrap(), StreamOrFutureEvent::Read { tx });
        let instance = self.instance;
        let id = self.id;
        Promise {
            inner: Box::pin(rx.map(|v| match v {
                Ok(HostReadResult::Values(v)) => Ok((self, v)),
                Err(_) => {
                    self.tx.take();
                    Err(None)
                }
                Ok(HostReadResult::EndOfStream(s)) => {
                    self.tx.take();
                    Err(s)
                }
            })),
            instance,
            id,
        }
    }

    /// Convert this object into a `Promise` which will resolve when the write
    /// end of this `stream` is closed, plus a `Watch` which can be used to
    /// retrieve the `StreamReader` again.
    ///
    /// Note that calling `Watch::into_inner` on the returned `Watch` will have
    /// the side effect of causing the `Promise` to resolve immediately if it
    /// hasn't already.
    pub fn watch_writer(mut self) -> (Promise<()>, Watch<Self>)
    where
        B: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchWriter { tx },
        );
        let instance = self.instance;
        let id = self.id;
        watch(instance, id, rx, self)
    }
}

impl<B: Send> Ready for StreamReader<B> {
    async fn ready(&mut self) {
        _ = future::poll_fn(|cx| self.tx.as_mut().unwrap().poll_ready(cx)).await;
    }
}

impl<B> Drop for StreamReader<B> {
    fn drop(&mut self) {
        if let Some(mut tx) = self.tx.take() {
            send(&mut tx, StreamOrFutureEvent::CloseReader);
        }
    }
}

/// Represents a Component Model `error-context`.
pub struct ErrorContext {
    rep: u32,
}

impl ErrorContext {
    pub(crate) fn new(rep: u32) -> Self {
        Self { rep }
    }

    /// Convert this `ErrorContext` into a [`Val`].
    pub fn into_val(self) -> Val {
        Val::ErrorContext(ErrorContextAny(self.rep))
    }

    /// Attempt to convert the specified [`Val`] to a `ErrorContext`.
    pub fn from_val<U, S: AsContextMut<Data = U>>(_: S, value: &Val) -> Result<Self> {
        let Val::ErrorContext(ErrorContextAny(rep)) = value else {
            bail!("expected `error-context`; got `{}`", value.desc());
        };
        Ok(Self::new(*rep))
    }

    fn lift_from_index(cx: &mut LiftContext<'_>, ty: InterfaceType, index: u32) -> Result<Self> {
        match ty {
            InterfaceType::ErrorContext(src) => {
                let (rep, _) = unsafe {
                    (*cx.instance)
                        .error_context_tables()
                        .get_mut(src)
                        .expect(
                            "error context table index present in (sub)component table during lift",
                        )
                        .get_mut_by_index(index)?
                };

                Ok(Self { rep })
            }
            _ => func::bad_type_info(),
        }
    }
}

pub(crate) fn lower_error_context_to_index<U>(
    rep: u32,
    cx: &mut LowerContext<'_, U>,
    ty: InterfaceType,
) -> Result<u32> {
    match ty {
        InterfaceType::ErrorContext(dst) => {
            let tbl = unsafe {
                &mut (*cx.instance).error_context_tables().get_mut(dst).expect(
                    "error context table index present in (sub)component table during lower",
                )
            };

            if let Some((dst_idx, dst_state)) = tbl.get_mut_by_rep(rep) {
                dst_state.0 += 1;
                Ok(dst_idx)
            } else {
                tbl.insert(rep, LocalErrorContextRefCount(1))
            }
        }
        _ => func::bad_type_info(),
    }
}

unsafe impl func::ComponentType for ErrorContext {
    const ABI: CanonicalAbiInfo = CanonicalAbiInfo::SCALAR4;

    type Lower = <u32 as func::ComponentType>::Lower;

    fn typecheck(ty: &InterfaceType, _types: &InstanceType<'_>) -> Result<()> {
        match ty {
            InterfaceType::ErrorContext(_) => Ok(()),
            other => bail!("expected `error`, found `{}`", func::desc(other)),
        }
    }
}

unsafe impl func::Lower for ErrorContext {
    fn lower<T>(
        &self,
        cx: &mut LowerContext<'_, T>,
        ty: InterfaceType,
        dst: &mut MaybeUninit<Self::Lower>,
    ) -> Result<()> {
        lower_error_context_to_index(self.rep, cx, ty)?.lower(cx, InterfaceType::U32, dst)
    }

    fn store<T>(
        &self,
        cx: &mut LowerContext<'_, T>,
        ty: InterfaceType,
        offset: usize,
    ) -> Result<()> {
        lower_error_context_to_index(self.rep, cx, ty)?.store(cx, InterfaceType::U32, offset)
    }
}

unsafe impl func::Lift for ErrorContext {
    fn lift(cx: &mut LiftContext<'_>, ty: InterfaceType, src: &Self::Lower) -> Result<Self> {
        let index = u32::lift(cx, InterfaceType::U32, src)?;
        Self::lift_from_index(cx, ty, index)
    }

    fn load(cx: &mut LiftContext<'_>, ty: InterfaceType, bytes: &[u8]) -> Result<Self> {
        let index = u32::load(cx, InterfaceType::U32, bytes)?;
        Self::lift_from_index(cx, ty, index)
    }
}

pub(super) struct TransmitHandle {
    pub(super) common: WaitableCommon,
    state: TableId<TransmitState>,
}

impl TransmitHandle {
    fn new(state: TableId<TransmitState>) -> Self {
        Self {
            common: WaitableCommon::default(),
            state,
        }
    }
}

struct TransmitState {
    write_handle: TableId<TransmitHandle>,
    read_handle: TableId<TransmitHandle>,
    write: WriteState,
    read: ReadState,
    writer_watcher: Option<oneshot::Sender<()>>,
    reader_watcher: Option<oneshot::Sender<()>>,
}

impl Default for TransmitState {
    fn default() -> Self {
        Self {
            write_handle: TableId::new(0),
            read_handle: TableId::new(0),
            read: ReadState::Open,
            write: WriteState::Open,
            reader_watcher: None,
            writer_watcher: None,
        }
    }
}

enum WriteState {
    Open,
    GuestReady {
        ty: TableIndex,
        flat_abi: Option<FlatAbi>,
        options: Options,
        address: usize,
        count: usize,
        handle: u32,
        post_write: PostWrite,
    },
    HostReady {
        accept: Box<dyn FnOnce(&mut ComponentInstance, Reader) -> Result<usize> + Send + Sync>,
        post_write: PostWrite,
    },
    Closed(Option<TypeComponentGlobalErrorContextTableIndex>),
}

/// Read state of a transmit channel
///
/// Channels generally start as open, and once they are read for data by either
/// a guest or host, we transition into `GuestReady` or `HostReady` respectively.
///
/// Once a transmit channel is closed, it should *stay* closed.
enum ReadState {
    Open,
    GuestReady {
        ty: TableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        flat_abi: Option<FlatAbi>,
        options: Options,
        address: usize,
        count: usize,
        handle: u32,
    },
    HostReady {
        accept: Box<dyn FnOnce(Writer) -> Result<usize> + Send + Sync>,
    },
    Closed,
}

enum Writer<'a> {
    /// Writes that are queued from guests
    Guest {
        lift: &'a mut LiftContext<'a>,
        ty: Option<InterfaceType>,
        address: usize,
        count: usize,
    },
    Host {
        values: Box<dyn Any>,
    },
    End(Option<TypeComponentGlobalErrorContextTableIndex>),
}

struct RawLowerContext<'a> {
    options: &'a Options,
}

enum Reader<'a> {
    Guest {
        lower: RawLowerContext<'a>,
        ty: TableIndex,
        address: usize,
        count: usize,
    },
    Host {
        accept: Box<dyn FnOnce(Box<dyn Any>) -> Result<()>>,
    },
}

impl Instance {
    /// Create a new Component Model `future` as pair of writable and readable ends,
    /// the latter of which may be passed to guest code.
    pub fn future<
        T: func::Lower + func::Lift + Sync + Send + 'static,
        U,
        S: AsContextMut<Data = U>,
    >(
        &self,
        mut store: S,
    ) -> Result<(FutureWriter<T>, FutureReader<T>)> {
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[self.0].as_ref().unwrap().instance_ptr() };
        let (write, read) = instance.new_transmit()?;

        Ok((
            FutureWriter::new(
                Some(instance.start_event_loop::<_, _, U>(write.rep())),
                instance,
            ),
            FutureReader::new(
                read.rep(),
                Some(instance.start_event_loop::<_, _, U>(read.rep())),
                instance,
            ),
        ))
    }

    /// Create a new Component Model `stream` as pair of writable and readable ends,
    /// the latter of which may be passed to guest code.
    pub fn stream<
        T: func::Lower + func::Lift + Sync + Send + 'static,
        B: Buffer<T>,
        U,
        S: AsContextMut<Data = U>,
    >(
        &self,
        mut store: S,
    ) -> Result<(StreamWriter<B>, StreamReader<B>)> {
        let store = store.as_context_mut();
        let instance = unsafe { &mut *store.0[self.0].as_ref().unwrap().instance_ptr() };
        let (write, read) = instance.new_transmit()?;

        Ok((
            StreamWriter::new(
                Some(instance.start_event_loop::<_, _, U>(write.rep())),
                instance,
            ),
            StreamReader::new(
                read.rep(),
                Some(instance.start_event_loop::<_, _, U>(read.rep())),
                instance,
            ),
        ))
    }
}

impl ComponentInstance {
    fn start_event_loop<T: func::Lower + func::Lift + Sync + Send + 'static, B: Buffer<T>, U>(
        &mut self,
        rep: u32,
    ) -> mpsc::Sender<StreamOrFutureEvent<B>> {
        fn write<T: func::Lower + Sync + Send + 'static, B: Buffer<T>, U>(
            instance: SendSyncPtr<ComponentInstance>,
            rep: u32,
            values: B,
            post_write: PostWrite,
            tx: oneshot::Sender<()>,
        ) -> Result<()> {
            let instance = unsafe { &mut *instance.as_ptr() };
            let store = unsafe { StoreContextMut::<U>(&mut *instance.store().cast()) };
            instance.host_write(store, rep, values, post_write, tx)
        }

        fn close_writer(
            instance: SendSyncPtr<ComponentInstance>,
            rep: u32,
            err_ctx: Option<ErrorContext>,
        ) -> Result<()> {
            let instance = unsafe { &mut *instance.as_ptr() };
            instance.host_close_writer(
                rep,
                err_ctx.map(|v| TypeComponentGlobalErrorContextTableIndex::from_u32(v.rep)),
            )
        }

        fn watch_writer(
            instance: SendSyncPtr<ComponentInstance>,
            rep: u32,
            tx: oneshot::Sender<()>,
        ) -> Result<()> {
            let instance = unsafe { &mut *instance.as_ptr() };
            let state = instance.get_mut(TableId::<TransmitState>::new(rep))?;
            if !matches!(&state.write, WriteState::Closed(_)) {
                state.writer_watcher = Some(tx);
            }
            Ok(())
        }

        fn read<T: func::Lift + Sync + Send + 'static, B: Buffer<T>, U>(
            instance: SendSyncPtr<ComponentInstance>,
            rep: u32,
            tx: oneshot::Sender<HostReadResult<B>>,
        ) -> Result<()> {
            let instance = unsafe { &mut *instance.as_ptr() };
            let store = unsafe { StoreContextMut::<U>(&mut *instance.store().cast()) };
            instance.host_read::<T, B, _, _>(store, rep, tx)
        }

        fn close_reader(instance: SendSyncPtr<ComponentInstance>, rep: u32) -> Result<()> {
            let instance = unsafe { &mut *instance.as_ptr() };
            instance.host_close_reader(rep)
        }

        fn watch_reader(
            instance: SendSyncPtr<ComponentInstance>,
            rep: u32,
            tx: oneshot::Sender<()>,
        ) -> Result<()> {
            let instance = unsafe { &mut *instance.as_ptr() };
            let state = instance.get_mut(TableId::<TransmitState>::new(rep))?;
            if !matches!(&state.read, ReadState::Closed) {
                state.reader_watcher = Some(tx);
            }
            Ok(())
        }

        fn get_state_rep(instance: SendSyncPtr<ComponentInstance>, rep: u32) -> Result<u32> {
            let instance = unsafe { &mut *instance.as_ptr() };
            Ok(instance
                .get(TableId::<TransmitHandle>::new(rep))
                .with_context(|| format!("stream or future rep {rep} not found"))?
                .state
                .rep())
        }

        let (tx, mut rx) = mpsc::channel(1);
        let task = Box::pin(
            {
                let instance = SendSyncPtr::new(NonNull::new(self).unwrap());
                async move {
                    let mut my_rep = None;
                    while let Some(event) = rx.next().await {
                        if my_rep.is_none() {
                            my_rep = Some(get_state_rep(instance, rep)?);
                        }
                        let rep = my_rep.unwrap();
                        match event {
                            StreamOrFutureEvent::Write {
                                values,
                                post_write,
                                tx,
                            } => write::<T, B, U>(instance, rep, values, post_write, tx)?,
                            StreamOrFutureEvent::CloseWriter { err_ctx } => {
                                close_writer(instance, rep, err_ctx)?
                            }
                            StreamOrFutureEvent::WatchWriter { tx } => {
                                watch_writer(instance, rep, tx)?
                            }
                            StreamOrFutureEvent::Read { tx } => read::<T, B, U>(instance, rep, tx)?,
                            StreamOrFutureEvent::CloseReader => close_reader(instance, rep)?,
                            StreamOrFutureEvent::WatchReader { tx } => {
                                watch_reader(instance, rep, tx)?
                            }
                        }
                    }
                    Ok(())
                }
            }
            .map(HostTaskOutput::Background),
        );
        self.push_future(task);
        tx
    }

    fn push_event(&mut self, rep: u32, event: Event, param: usize) {
        log::trace!("push event {event:?} for {rep}");
        self.push_future(Box::pin(future::ready(HostTaskOutput::Call {
            waitable: rep,
            fun: Box::new(move |_| {
                Ok(HostTaskResult {
                    event,
                    param: u32::try_from(param).unwrap(),
                })
            }),
        })) as HostTaskFuture);
    }

    fn get_mut_by_index(
        &mut self,
        ty: TableIndex,
        index: u32,
    ) -> Result<(u32, &mut StreamFutureState)> {
        get_mut_by_index_from(self.state_table(ty), ty, index)
    }

    fn new_transmit(&mut self) -> Result<(TableId<TransmitHandle>, TableId<TransmitHandle>)> {
        let state_id = self.push(TransmitState::default())?;
        let write = self.push(TransmitHandle::new(state_id))?;
        let read = self.push(TransmitHandle::new(state_id))?;
        let state = self.get_mut(state_id)?;
        state.write_handle = write;
        state.read_handle = read;

        log::trace!(
            "new transmit: state {}; write {}; read {}",
            state_id.rep(),
            write.rep(),
            read.rep(),
        );

        Ok((write, read))
    }

    fn delete_transmit(&mut self, state_id: TableId<TransmitState>) -> Result<()> {
        let state = self.delete(state_id)?;
        self.delete(state.write_handle)?;
        self.delete(state.read_handle)?;

        log::trace!(
            "delete transmit: state {}; write {}; read {}",
            state_id.rep(),
            state.write_handle.rep(),
            state.read_handle.rep(),
        );

        Ok(())
    }

    fn state_table(&mut self, ty: TableIndex) -> &mut StateTable<WaitableState> {
        let runtime_instance = match ty {
            TableIndex::Stream(ty) => self.component_types()[ty].instance,
            TableIndex::Future(ty) => self.component_types()[ty].instance,
        };
        &mut self.waitable_tables()[runtime_instance]
    }

    fn guest_new(&mut self, ty: TableIndex) -> Result<u32> {
        let state = self.new_transmit()?.0.rep();
        self.state_table(ty)
            .insert(state, waitable_state(ty, StreamFutureState::Local))
    }

    /// Write to a waitable from the host
    ///
    /// # Arguments
    ///
    /// * `store` - The engine store
    /// * `transmit_rep` - Global representation of the transmit object that will be modified
    /// * `values` - List of values that should be written
    /// * `post_write` - Whether the transmit should be closed after write, possibly with an error context
    /// * `tx` - Oneshot channel to notify when operation completes (or drop on error)
    ///
    fn host_write<
        T: func::Lower + Send + Sync + 'static,
        B: Buffer<T>,
        U,
        S: AsContextMut<Data = U>,
    >(
        &mut self,
        mut store: S,
        transmit_rep: u32,
        values: B,
        mut post_write: PostWrite,
        tx: oneshot::Sender<()>,
    ) -> Result<()> {
        let mut store = store.as_context_mut();
        let transmit_id = TableId::<TransmitState>::new(transmit_rep);
        let mut offset = 0;

        let mut tx = Some(tx);

        loop {
            let transmit = self
                .get_mut(transmit_id)
                .with_context(|| format!("retrieving state for transmit [{transmit_rep}]"))?;

            let new_state = if let ReadState::Closed = &transmit.read {
                ReadState::Closed
            } else {
                ReadState::Open
            };

            match mem::replace(&mut transmit.read, new_state) {
                ReadState::Open => {
                    assert!(matches!(&transmit.write, WriteState::Open));

                    transmit.write = WriteState::HostReady {
                        accept: Box::new(accept::<T, B, U>(
                            values,
                            offset,
                            transmit_id,
                            tx.take().unwrap(),
                        )),
                        post_write,
                    };
                    post_write = PostWrite::Continue;
                }

                ReadState::GuestReady {
                    ty,
                    flat_abi: _,
                    options,
                    address,
                    count,
                    handle,
                    ..
                } => {
                    let read_handle = transmit.read_handle;
                    let instance = self as *mut _;
                    let types = self.component_types();
                    let lower = unsafe {
                        &mut LowerContext::new(store.as_context_mut(), &options, types, instance)
                    };
                    if address % usize::try_from(T::ALIGN32)? != 0 {
                        bail!("read pointer not aligned");
                    }
                    lower
                        .as_slice_mut()
                        .get_mut(address..)
                        .and_then(|b| b.get_mut(..T::SIZE32 * count))
                        .ok_or_else(|| anyhow::anyhow!("read pointer out of bounds of memory"))?;

                    let count = values.len().min(count);
                    if let Some(ty) = payload(ty, types) {
                        T::store_list(lower, ty, address, &values[offset..][..count])?;
                    }
                    offset += count;

                    *self.get_mut_by_index(ty, handle)?.1 = StreamFutureState::Read;

                    self.push_event(
                        read_handle.rep(),
                        match ty {
                            TableIndex::Future(_) => Event::FutureRead,
                            TableIndex::Stream(_) => Event::StreamRead,
                        },
                        count,
                    );

                    if offset < values.len() {
                        continue;
                    }
                }

                ReadState::HostReady { accept } => {
                    accept(Writer::Host {
                        values: Box::new(values),
                    })?;
                }

                ReadState::Closed => {
                    self.delete_transmit(transmit_id)?;
                    break Ok(());
                }
            }

            if let PostWrite::Close(err_ctx) = post_write {
                self.host_close_writer(transmit_rep, err_ctx)?;
            }

            if let Some(tx) = tx.take() {
                _ = tx.send(());
            }

            break Ok(());
        }
    }

    fn host_read<
        T: func::Lift + Sync + Send + 'static,
        B: Buffer<T>,
        U,
        S: AsContextMut<Data = U>,
    >(
        &mut self,
        mut store: S,
        rep: u32,
        tx: oneshot::Sender<HostReadResult<B>>,
    ) -> Result<()> {
        let store = store.as_context_mut();
        let transmit_id = TableId::<TransmitState>::new(rep);
        let transmit = self.get_mut(transmit_id).with_context(|| rep.to_string())?;

        let new_state = if let WriteState::Closed(maybe_err_ctx) = &transmit.write {
            WriteState::Closed(*maybe_err_ctx)
        } else {
            WriteState::Open
        };

        match mem::replace(&mut transmit.write, new_state) {
            WriteState::Open => {
                assert!(matches!(&transmit.read, ReadState::Open));

                transmit.read = ReadState::HostReady {
                    accept: Box::new(move |writer| {
                        Ok(match writer {
                            Writer::Guest {
                                lift,
                                ty,
                                address,
                                count,
                            } => {
                                _ = tx.send(HostReadResult::Values(if T::IS_RUST_UNIT_TYPE {
                                    B::from(
                                        iter::repeat_with(|| unsafe {
                                            MaybeUninit::uninit().assume_init()
                                        })
                                        .take(count)
                                        .collect(),
                                    )
                                } else {
                                    let ty = ty.unwrap();
                                    if address % usize::try_from(T::ALIGN32)? != 0 {
                                        bail!("write pointer not aligned");
                                    }
                                    lift.memory()
                                        .get(address..)
                                        .and_then(|b| b.get(..T::SIZE32 * count))
                                        .ok_or_else(|| {
                                            anyhow::anyhow!("write pointer out of bounds of memory")
                                        })?;

                                    let list = &WasmList::new(address, count, lift, ty)?;
                                    B::from(T::load_list(lift, list)?)
                                }));
                                count
                            }
                            Writer::Host { values } => {
                                let values = *values
                                    .downcast::<B>()
                                    .map_err(|_| anyhow!("transmit type mismatch"))?;
                                let count = values.len();
                                _ = tx.send(HostReadResult::Values(values));
                                count
                            }
                            Writer::End(maybe_err_ctx) => match maybe_err_ctx {
                                None => {
                                    _ = tx.send(HostReadResult::EndOfStream(None));
                                    CLOSED
                                }
                                Some(err_ctx) => {
                                    _ = tx.send(HostReadResult::EndOfStream(Some(
                                        ErrorContext::new(err_ctx.as_u32()),
                                    )));
                                    CLOSED | err_ctx.as_u32() as usize
                                }
                            },
                        })
                    }),
                };
            }

            WriteState::GuestReady {
                ty,
                flat_abi: _,
                options,
                address,
                count,
                handle,
                post_write,
                ..
            } => {
                let write_handle = transmit.write_handle;
                let instance = self as *mut _;
                let types = self.component_types();
                let lift = unsafe { &mut LiftContext::new(store.0, &options, types, instance) };
                _ = tx.send(HostReadResult::Values(if T::IS_RUST_UNIT_TYPE {
                    B::from(
                        iter::repeat_with(|| unsafe { MaybeUninit::uninit().assume_init() })
                            .take(count)
                            .collect(),
                    )
                } else {
                    let ty = payload(ty, types).unwrap();
                    let list = &WasmList::new(address, count, lift, ty)?;
                    B::from(T::load_list(lift, list)?)
                }));

                if let PostWrite::Close(err_ctx) = post_write {
                    self.get_mut(transmit_id)?.write = WriteState::Closed(err_ctx);
                } else {
                    *self.get_mut_by_index(ty, handle)?.1 = StreamFutureState::Write;
                }

                self.push_event(
                    write_handle.rep(),
                    match ty {
                        TableIndex::Future(_) => Event::FutureWrite,
                        TableIndex::Stream(_) => Event::StreamWrite,
                    },
                    count,
                );
            }

            WriteState::HostReady { accept, post_write } => {
                accept(
                    self,
                    Reader::Host {
                        accept: Box::new(move |any| {
                            _ = tx.send(HostReadResult::Values(
                                *any.downcast()
                                    .map_err(|_| anyhow!("transmit type mismatch"))?,
                            ));
                            Ok(())
                        }),
                    },
                )?;

                if let PostWrite::Close(err_ctx) = post_write {
                    self.get_mut(transmit_id)?.write = WriteState::Closed(err_ctx);
                }
            }

            WriteState::Closed(_) => {
                self.host_close_reader(rep)?;
            }
        }

        Ok(())
    }

    fn host_cancel_write(&mut self, rep: u32) -> Result<u32> {
        let transmit_id = TableId::<TransmitState>::new(rep);
        let transmit = self.get_mut(transmit_id)?;

        match &transmit.write {
            WriteState::GuestReady { .. } | WriteState::HostReady { .. } => {
                transmit.write = WriteState::Open;
            }

            WriteState::Open | WriteState::Closed(_) => {
                bail!("stream or future write canceled when no write is pending")
            }
        }

        log::trace!("canceled write {rep}");

        Ok(0)
    }

    fn host_cancel_read(&mut self, rep: u32) -> Result<u32> {
        let transmit_id = TableId::<TransmitState>::new(rep);
        let transmit = self.get_mut(transmit_id)?;

        match &transmit.read {
            ReadState::GuestReady { .. } | ReadState::HostReady { .. } => {
                transmit.read = ReadState::Open;
            }

            ReadState::Open | ReadState::Closed => {
                bail!("stream or future read canceled when no read is pending")
            }
        }

        log::trace!("canceled read {rep}");

        Ok(0)
    }

    /// Close the writer end of a Future or Stream
    ///
    /// # Arguments
    ///
    /// * `transmit_rep` - A component-global representation of the transmit state for the writer that should be closed
    /// * `err_ctx` - An optional component-global representation of an error context to use as the final value of the writer
    ///
    fn host_close_writer(
        &mut self,
        transmit_rep: u32,
        err_ctx: Option<TypeComponentGlobalErrorContextTableIndex>,
    ) -> Result<()> {
        let transmit_id = TableId::<TransmitState>::new(transmit_rep);
        let transmit = self
            .get_mut(transmit_id)
            .with_context(|| format!("writer {transmit_rep}"))?;

        transmit.writer_watcher = None;

        // Existing queued transmits must be updated with information for the impending writer closure
        match &mut transmit.write {
            WriteState::GuestReady { post_write, .. } => {
                *post_write = PostWrite::Close(err_ctx);
            }
            WriteState::HostReady { post_write, .. } => {
                *post_write = PostWrite::Close(err_ctx);
            }
            v @ WriteState::Open => {
                *v = WriteState::Closed(err_ctx);
            }
            WriteState::Closed(_) => unreachable!("write state is already closed"),
        }

        // If the existing read state is closed, then there's nothing to read
        // and we can keep it that way.
        //
        // If the read state was any other state, then we must set the new state to open
        // to indicate that there *is* data to be read
        let new_state = if let ReadState::Closed = &transmit.read {
            ReadState::Closed
        } else {
            ReadState::Open
        };

        // Swap in the new read state
        match mem::replace(&mut transmit.read, new_state) {
            // If the guest was ready to read, then we cannot close the reader (or writer)
            // we must deliver the event, and update the state associated with the handle to
            // represent that a read must be performed
            ReadState::GuestReady {
                ty,
                err_ctx_ty,
                handle,
                ..
            } => {
                let read_handle = transmit.read_handle;

                // Lift the global err_ctx that we're receiving into an error context
                // reference that the reader(caller) will
                let reader_state_tbl = self
                    .error_context_tables()
                    .get_mut(err_ctx_ty)
                    .context("retrieving component-local error context during host writer close")?;

                let push_param = match err_ctx {
                    None => CLOSED,
                    Some(err_ctx) => {
                        let rep = err_ctx.as_u32();
                        // Get or insert the global error context into this guest's component-local error context tracking
                        let (local_err_ctx, _) = match reader_state_tbl.get_mut_by_rep(rep) {
                            Some(r) => {
                                // If the error already existed, since we're about to read it, increase
                                // the local component-wide reference count
                                (*r.1).0 += 1;
                                r
                            }
                            None => {
                                // If the error context was not already tracked locally, start tracking
                                reader_state_tbl.insert(rep, LocalErrorContextRefCount(1))?;
                                reader_state_tbl.get_mut_by_rep(rep).context(
                                    "retrieving inserted local error context during guest read",
                                )?
                            }
                        };

                        // NOTE: we do not have to manage the global error context ref count here, because
                        // it was preemptively increased, and the guest that is ready to consume this
                        // will account for the extra global context ref count.

                        CLOSED | local_err_ctx as usize
                    }
                };

                // Ensure the final read of the guest is queued, with appropriate closure indicator
                self.push_event(
                    read_handle.rep(),
                    match ty {
                        TableIndex::Future(_) => Event::FutureRead,
                        TableIndex::Stream(_) => Event::StreamRead,
                    },
                    push_param,
                );

                *self.get_mut_by_index(ty, handle)?.1 = StreamFutureState::Read;
            }

            // If the host was ready to read, and the writer end is being closed (host->host write?)
            // signal to the reader that we've reached the end of the stream, and close the reader immediately
            ReadState::HostReady { accept } => {
                accept(Writer::End(err_ctx))?;
                self.host_close_reader(transmit_rep)?;
            }

            // If the read state is open, then there are no registered readers of the stream/future
            ReadState::Open => {}

            // If the read state was already closed, then we can remove the transmit state completely
            // (both writer and reader have been closed)
            ReadState::Closed => {
                log::trace!("host_close_writer delete {transmit_rep}");
                self.delete_transmit(transmit_id)?;
            }
        }
        Ok(())
    }

    /// Close the reader end of a Future or Stream
    ///
    /// # Arguments
    ///
    /// * `transmit_rep` - A global-component-level representation of the transmit state for the reader that should be closed
    ///
    fn host_close_reader(&mut self, transmit_rep: u32) -> Result<()> {
        let transmit_id = TableId::<TransmitState>::new(transmit_rep);
        let transmit = self
            .get_mut(transmit_id)
            .with_context(|| format!("reader {transmit_rep}"))?;

        transmit.read = ReadState::Closed;
        transmit.reader_watcher = None;

        // If the write end is already closed, it should stay closed,
        // otherwise, it should be opened.
        let new_state = if let WriteState::Closed(err_ctx) = &transmit.write {
            WriteState::Closed(*err_ctx)
        } else {
            WriteState::Open
        };

        match mem::replace(&mut transmit.write, new_state) {
            // If a guest is waiting to write, ensure that the next write
            // reflects the closed state of the stream
            WriteState::GuestReady {
                ty,
                handle,
                post_write,
                ..
            } => {
                let read_handle = transmit.read_handle;
                self.push_event(
                    read_handle.rep(),
                    match ty {
                        TableIndex::Future(_) => Event::FutureRead,
                        TableIndex::Stream(_) => Event::StreamRead,
                    },
                    CLOSED,
                );

                if let PostWrite::Close(_) = post_write {
                    self.delete_transmit(transmit_id)?;
                } else {
                    *self.get_mut_by_index(ty, handle)?.1 = StreamFutureState::Write;
                }
            }

            // If the reader is closed, we can ignore the waiting write from  host
            WriteState::HostReady { .. } => {
                self.delete_transmit(transmit_id)?;
            }

            WriteState::Open => {}

            WriteState::Closed(_) => {
                log::trace!("host_close_reader delete {transmit_rep}");
                self.delete_transmit(transmit_id)?;
            }
        }
        Ok(())
    }

    fn copy<T>(
        &mut self,
        mut store: StoreContextMut<'_, T>,
        flat_abi: Option<FlatAbi>,
        write_ty: TableIndex,
        write_options: &Options,
        write_address: usize,
        read_ty: TableIndex,
        read_options: &Options,
        read_address: usize,
        count: usize,
        rep: u32,
    ) -> Result<()> {
        match (write_ty, read_ty) {
            (TableIndex::Future(write_ty), TableIndex::Future(read_ty)) => {
                assert_eq!(count, 1);

                let instance = self as *mut _;
                let types = self.component_types();
                let val = types[types[write_ty].ty]
                    .payload
                    .map(|ty| {
                        let abi = types.canonical_abi(&ty);
                        // FIXME: needs to read an i64 for memory64
                        if write_address % usize::try_from(abi.align32)? != 0 {
                            bail!("write pointer not aligned");
                        }

                        let lift = unsafe {
                            &mut LiftContext::new(store.0, write_options, types, instance)
                        };

                        let bytes = lift
                            .memory()
                            .get(write_address..)
                            .and_then(|b| b.get(..usize::try_from(abi.size32).unwrap()))
                            .ok_or_else(|| {
                                anyhow::anyhow!("write pointer out of bounds of memory")
                            })?;

                        Val::load(lift, ty, bytes)
                    })
                    .transpose()?;

                if let Some(val) = val {
                    let lower = unsafe {
                        &mut LowerContext::new(
                            store.as_context_mut(),
                            read_options,
                            types,
                            instance,
                        )
                    };
                    let ty = types[types[read_ty].ty].payload.unwrap();
                    let ptr = func::validate_inbounds_dynamic(
                        types.canonical_abi(&ty),
                        lower.as_slice_mut(),
                        &ValRaw::u32(read_address.try_into().unwrap()),
                    )?;
                    val.store(lower, ty, ptr)?;
                }
            }
            (TableIndex::Stream(write_ty), TableIndex::Stream(read_ty)) => {
                let instance = self as *mut _;
                let types = self.component_types();
                let lift =
                    unsafe { &mut LiftContext::new(store.0, write_options, types, instance) };
                if let Some(flat_abi) = flat_abi {
                    // Fast path memcpy for "flat" (i.e. no pointers or handles) payloads:
                    let length_in_bytes = usize::try_from(flat_abi.size).unwrap() * count;
                    if length_in_bytes > 0 {
                        if write_address % usize::try_from(flat_abi.align)? != 0 {
                            bail!("write pointer not aligned");
                        }
                        if read_address % usize::try_from(flat_abi.align)? != 0 {
                            bail!("read pointer not aligned");
                        }

                        {
                            let src = write_options
                                .memory(store.0)
                                .get(write_address..)
                                .and_then(|b| b.get(..length_in_bytes))
                                .ok_or_else(|| {
                                    anyhow::anyhow!("write pointer out of bounds of memory")
                                })?
                                .as_ptr();
                            let dst = read_options
                                .memory_mut(store.0)
                                .get_mut(read_address..)
                                .and_then(|b| b.get_mut(..length_in_bytes))
                                .ok_or_else(|| {
                                    anyhow::anyhow!("read pointer out of bounds of memory")
                                })?
                                .as_mut_ptr();
                            unsafe { src.copy_to(dst, length_in_bytes) };
                        }
                    }
                } else {
                    let ty = types[types[write_ty].ty].payload.unwrap();
                    let abi = lift.types.canonical_abi(&ty);
                    let size = usize::try_from(abi.size32).unwrap();
                    if write_address % usize::try_from(abi.align32)? != 0 {
                        bail!("write pointer not aligned");
                    }
                    let bytes = lift
                        .memory()
                        .get(write_address..)
                        .and_then(|b| b.get(..size * count))
                        .ok_or_else(|| anyhow::anyhow!("write pointer out of bounds of memory"))?;

                    let values = (0..count)
                        .map(|index| Val::load(lift, ty, &bytes[(index * size)..][..size]))
                        .collect::<Result<Vec<_>>>()?;

                    log::trace!("copy values {values:?} for {rep}");

                    let lower = unsafe {
                        &mut LowerContext::new(
                            store.as_context_mut(),
                            read_options,
                            types,
                            instance,
                        )
                    };
                    let ty = types[types[read_ty].ty].payload.unwrap();
                    let abi = lower.types.canonical_abi(&ty);
                    if read_address % usize::try_from(abi.align32)? != 0 {
                        bail!("read pointer not aligned");
                    }
                    let size = usize::try_from(abi.size32).unwrap();
                    lower
                        .as_slice_mut()
                        .get_mut(read_address..)
                        .and_then(|b| b.get_mut(..size * count))
                        .ok_or_else(|| anyhow::anyhow!("read pointer out of bounds of memory"))?;
                    let mut ptr = read_address;
                    for value in values {
                        value.store(lower, ty, ptr)?;
                        ptr += size
                    }
                }
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub(super) fn guest_write<T>(
        &mut self,
        mut store: StoreContextMut<T>,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        flat_abi: Option<FlatAbi>,
        handle: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        // TODO: handle errors sent via `{stream|future}.close-readable`:
        _ = err_ctx_ty;

        let address = usize::try_from(address).unwrap();
        let count = usize::try_from(count).unwrap();
        let options = unsafe {
            Options::new(
                store.0.id(),
                NonNull::new(memory),
                NonNull::new(realloc),
                StringEncoding::from_u8(string_encoding).unwrap(),
                true,
                None,
            )
        };
        let (rep, state) = self.get_mut_by_index(ty, handle)?;
        let StreamFutureState::Write = *state else {
            bail!(
                "invalid handle {handle}; expected {:?}; got {:?}",
                StreamFutureState::Write,
                *state
            );
        };
        *state = StreamFutureState::Busy;
        let transmit_id = self.get(TableId::<TransmitHandle>::new(rep))?.state;
        log::trace!(
            "guest_write {rep} (handle {handle}; state {})",
            transmit_id.rep()
        );
        let transmit = self.get_mut(transmit_id)?;
        let new_state = if let ReadState::Closed = &transmit.read {
            ReadState::Closed
        } else {
            ReadState::Open
        };

        let result = match mem::replace(&mut transmit.read, new_state) {
            ReadState::GuestReady {
                ty: read_ty,
                flat_abi: read_flat_abi,
                options: read_options,
                address: read_address,
                count: read_count,
                handle: read_handle,
                ..
            } => {
                assert_eq!(flat_abi, read_flat_abi);

                let read_handle_rep = transmit.read_handle.rep();

                let count = count.min(read_count);

                self.copy(
                    store.as_context_mut(),
                    flat_abi,
                    ty,
                    &options,
                    address,
                    read_ty,
                    &read_options,
                    read_address,
                    count,
                    rep,
                )?;

                *self.get_mut_by_index(read_ty, read_handle)?.1 = StreamFutureState::Read;

                self.push_event(
                    read_handle_rep,
                    match read_ty {
                        TableIndex::Future(_) => Event::FutureRead,
                        TableIndex::Stream(_) => Event::StreamRead,
                    },
                    count,
                );

                count
            }

            ReadState::HostReady { accept } => {
                let instance = self as *mut _;
                let types = self.component_types();
                let lift = unsafe { &mut LiftContext::new(store.0, &options, types, instance) };
                accept(Writer::Guest {
                    lift,
                    ty: payload(ty, types),
                    address,
                    count,
                })?
            }

            ReadState::Open => {
                assert!(matches!(&transmit.write, WriteState::Open));

                let transmit = self.get_mut(transmit_id)?;
                transmit.write = WriteState::GuestReady {
                    ty,
                    flat_abi,
                    options,
                    address: usize::try_from(address).unwrap(),
                    count: usize::try_from(count).unwrap(),
                    handle,
                    post_write: PostWrite::Continue,
                };

                BLOCKED
            }

            ReadState::Closed => CLOSED,
        };

        if result != BLOCKED {
            *self.get_mut_by_index(ty, handle)?.1 = StreamFutureState::Write;
        }

        Ok(u32::try_from(result).unwrap())
    }

    pub(super) fn guest_read<T>(
        &mut self,
        mut store: StoreContextMut<T>,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        flat_abi: Option<FlatAbi>,
        handle: u32,
        address: u32,
        count: u32,
    ) -> Result<u32> {
        let address = usize::try_from(address).unwrap();
        let count = usize::try_from(count).unwrap();
        let options = unsafe {
            Options::new(
                store.0.id(),
                NonNull::new(memory),
                NonNull::new(realloc),
                StringEncoding::from_u8(string_encoding).unwrap(),
                true,
                None,
            )
        };
        let (rep, state) = self.get_mut_by_index(ty, handle)?;
        let StreamFutureState::Read = *state else {
            bail!(
                "invalid handle {handle}; expected {:?}; got {:?}",
                StreamFutureState::Read,
                *state
            );
        };
        *state = StreamFutureState::Busy;
        let transmit_id = self.get(TableId::<TransmitHandle>::new(rep))?.state;
        log::trace!(
            "guest_read {rep} (handle {handle}; state {})",
            transmit_id.rep()
        );
        let transmit = self.get_mut(transmit_id)?;
        let new_state = if let WriteState::Closed(err_ctx) = &transmit.write {
            WriteState::Closed(*err_ctx)
        } else {
            WriteState::Open
        };

        let result = match mem::replace(&mut transmit.write, new_state) {
            WriteState::GuestReady {
                ty: write_ty,
                flat_abi: write_flat_abi,
                options: write_options,
                address: write_address,
                count: write_count,
                handle: write_handle,
                post_write,
            } => {
                assert_eq!(flat_abi, write_flat_abi);

                let write_handle_rep = transmit.write_handle.rep();

                let count = count.min(write_count);

                self.copy(
                    store.as_context_mut(),
                    flat_abi,
                    write_ty,
                    &write_options,
                    write_address,
                    ty,
                    &options,
                    address,
                    count,
                    rep,
                )?;

                if let PostWrite::Close(err_ctx) = post_write {
                    self.get_mut(transmit_id)?.write = WriteState::Closed(err_ctx);
                } else {
                    *self.get_mut_by_index(write_ty, write_handle)?.1 = StreamFutureState::Write;
                }

                self.push_event(
                    write_handle_rep,
                    match write_ty {
                        TableIndex::Future(_) => Event::FutureWrite,
                        TableIndex::Stream(_) => Event::StreamWrite,
                    },
                    count,
                );

                count
            }

            WriteState::HostReady { accept, post_write } => {
                let count = accept(
                    self,
                    Reader::Guest {
                        lower: RawLowerContext { options: &options },
                        ty,
                        address: usize::try_from(address).unwrap(),
                        count,
                    },
                )?;

                if let PostWrite::Close(err_ctx) = post_write {
                    self.get_mut(transmit_id)?.write = WriteState::Closed(err_ctx);
                }

                count
            }

            WriteState::Open => {
                assert!(matches!(&transmit.read, ReadState::Open));

                let transmit = self.get_mut(transmit_id)?;
                transmit.read = ReadState::GuestReady {
                    ty,
                    flat_abi,
                    options,
                    address: usize::try_from(address).unwrap(),
                    count: usize::try_from(count).unwrap(),
                    handle,
                    err_ctx_ty,
                };

                BLOCKED
            }

            WriteState::Closed(err_ctx) => {
                match err_ctx {
                    // If no error context is provided, closed can be sent
                    None => CLOSED,
                    // If an error context was present, we must ensure it's created and bitwise OR w/ CLOSED
                    Some(err_ctx) => {
                        // Lower the global error context that was saved into write state into a component-local
                        // error context handle
                        let state_tbl = self.error_context_tables().get_mut(err_ctx_ty).context(
                            "retrieving local error context table during closed read w/ error",
                        )?;

                        // Get or insert the global error context into this guest's component-local error context tracking
                        let (local_err_ctx, _) = match state_tbl.get_mut_by_rep(err_ctx.as_u32()) {
                            Some(r) => {
                                // If the error already existed, since we're about to read it, increase
                                // the local component-wide reference count
                                (*r.1).0 += 1;
                                r
                            }
                            None => {
                                let rep = err_ctx.as_u32();
                                // If the error context was not already tracked locally, start tracking
                                state_tbl.insert(rep, LocalErrorContextRefCount(1))?;
                                state_tbl.get_mut_by_rep(rep).context(
                                    "retrieving inserted local error context during guest read",
                                )?
                            }
                        };

                        // NOTE: During write closure when the error context was provided, we
                        // incremented the global count to ensure the error context would not be garbage collected,
                        // if dropped by the sending component.
                        //
                        // Since we did that preemptively, we do not need to increment the global ref count even
                        // after this increase in local ref count.
                        //
                        // If a reader (this reader) *never* comes along, when the relevant stream/future is closed,
                        // the writer state will indicate that the global count must be amended.

                        // We reset the write state to a simple closed now that the error value has been read out,
                        // in the case that another read is performed
                        self.get_mut(transmit_id)?.write = WriteState::Closed(None);

                        CLOSED | local_err_ctx as usize
                    }
                }
            }
        };

        if result != BLOCKED {
            *self.get_mut_by_index(ty, handle)?.1 = StreamFutureState::Read;
        }

        Ok(u32::try_from(result).unwrap())
    }

    fn guest_cancel_write(&mut self, ty: TableIndex, writer: u32, _async_: bool) -> Result<u32> {
        let (rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
            self.state_table(ty).get_mut_by_index(writer)?
        else {
            bail!("invalid stream or future handle");
        };
        match state {
            StreamFutureState::Local | StreamFutureState::Write => {
                bail!("stream or future write canceled when no write is pending")
            }
            StreamFutureState::Read => {
                bail!("passed read end to `{{stream|future}}.cancel-write`")
            }
            StreamFutureState::Busy => {
                *state = StreamFutureState::Write;
            }
        }
        let rep = self.get(TableId::<TransmitHandle>::new(rep))?.state.rep();
        self.host_cancel_write(rep)
    }

    fn guest_cancel_read(&mut self, ty: TableIndex, reader: u32, _async_: bool) -> Result<u32> {
        let (rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
            self.state_table(ty).get_mut_by_index(reader)?
        else {
            bail!("invalid stream or future handle");
        };
        match state {
            StreamFutureState::Local | StreamFutureState::Read => {
                bail!("stream or future read canceled when no read is pending")
            }
            StreamFutureState::Write => {
                bail!("passed write end to `{{stream|future}}.cancel-read`")
            }
            StreamFutureState::Busy => {
                *state = StreamFutureState::Read;
            }
        }
        let rep = self.get(TableId::<TransmitHandle>::new(rep))?.state.rep();
        self.host_cancel_read(rep)
    }

    fn guest_close_writable(
        &mut self,
        ty: TableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        local_err_ctx: u32,
    ) -> Result<()> {
        let (transmit_rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
            self.state_table(ty)
                .remove_by_index(writer)
                .context("failed to find writer")?
        else {
            bail!("invalid stream or future handle");
        };
        match state {
            StreamFutureState::Local | StreamFutureState::Write => {}
            StreamFutureState::Read => {
                bail!("passed read end to `{{stream|future}}.close-writable`")
            }
            StreamFutureState::Busy => bail!("cannot drop busy stream or future"),
        }

        // Resolve the error context
        let global_err_ctx = match local_err_ctx {
            // If no error context was provided, we can pass that along as-is
            0 => None,

            // If a non-zero error context was provided, first ensure it's valid,
            // then lift the guest-local (component instance local) error context reference
            // to the component-global level.
            //
            // This ensures that after closing the writer, when the eventual reader appears
            // we can lower the component-global error context into a reader-local error context
            err_ctx => {
                // Look up the local component error context
                let state_tbl = self
                    .error_context_tables()
                    .get_mut(err_ctx_ty)
                    .context("retrieving local error context during guest close writable")?;

                // NOTE: the rep below is the component-global error context index
                let (rep, _) = state_tbl.get_mut_by_index(local_err_ctx).with_context(|| {
                format!("missing component local error context idx [{local_err_ctx}] while closing writable")
            })?;

                let global_err_ctx = TypeComponentGlobalErrorContextTableIndex::from_u32(rep);

                // Closing the writer with an error context means that a reader must later
                // come along and discover the error context even once the writer goes away.
                //
                // Here we preemptively increase the ref count to ensure the error context
                // won't be removed by the time the reader comes along
                let GlobalErrorContextRefCount(global_count) = self
                    .global_error_context_ref_counts()
                    .get_mut(&global_err_ctx)
                    .context(
                        "retrieving global error context ref count during guest close writable",
                    )?;
                *global_count += 1;
                ensure!(
                self
                    .get(TableId::<ErrorContextState>::new(rep))
                    .is_ok(),
                "missing global error context state [{rep}] for local error context [{err_ctx}] during guest close writable"
            );
                Some(global_err_ctx)
            }
        };

        let transmit_rep = self
            .get(TableId::<TransmitHandle>::new(transmit_rep))?
            .state
            .rep();
        self.host_close_writer(transmit_rep, global_err_ctx)
    }

    fn guest_close_readable(
        &mut self,
        ty: TableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        reader: u32,
        error: u32,
    ) -> Result<()> {
        if error != 0 {
            _ = err_ctx_ty;
            todo!();
        }

        let (rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
            self.state_table(ty).remove_by_index(reader)?
        else {
            bail!("invalid stream or future handle");
        };
        match state {
            StreamFutureState::Local | StreamFutureState::Read => {}
            StreamFutureState::Write => {
                bail!("passed write end to `{{stream|future}}.close-readable`")
            }
            StreamFutureState::Busy => bail!("cannot drop busy stream or future"),
        }
        let rep = self.get(TableId::<TransmitHandle>::new(rep))?.state.rep();
        self.host_close_reader(rep)
    }

    /// Create a new error context for the given component
    pub(crate) fn error_context_new(
        &mut self,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        debug_msg_address: u32,
        debug_msg_len: u32,
    ) -> Result<u32> {
        //  Read string from guest memory
        let options = unsafe {
            Options::new(
                (*self.store()).store_opaque().id(),
                NonNull::new(memory),
                NonNull::new(realloc),
                StringEncoding::from_u8(string_encoding).ok_or_else(|| {
                    anyhow::anyhow!("failed to convert u8 string encoding [{string_encoding}]")
                })?,
                false,
                None,
            )
        };
        let interface = self as *mut _;
        let lift_ctx = unsafe {
            &mut LiftContext::new(
                (*self.store()).store_opaque_mut(),
                &options,
                self.component_types(),
                interface,
            )
        };
        let s = {
            let address = usize::try_from(debug_msg_address)?;
            let len = usize::try_from(debug_msg_len)?;
            WasmStr::load(
                lift_ctx,
                InterfaceType::String,
                &lift_ctx
                    .memory()
                    .get(address..)
                    .and_then(|b| b.get(..len))
                    .map(|_| {
                        [debug_msg_address.to_le_bytes(), debug_msg_len.to_le_bytes()].concat()
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("invalid debug message pointer: out of bounds")
                    })?,
            )?
        };

        // Create a new ErrorContext that is tracked along with other concurrent state
        let err_ctx = ErrorContextState {
            debug_msg: s
                .to_str_from_memory(options.memory(unsafe { (*self.store()).store_opaque() }))?
                .to_string(),
        };
        let table_id = self.push(err_ctx)?;
        let global_ref_count_idx =
            TypeComponentGlobalErrorContextTableIndex::from_u32(table_id.rep());

        // Add to the global error context ref counts
        let _ = self
            .global_error_context_ref_counts()
            .insert(global_ref_count_idx, GlobalErrorContextRefCount(1));

        // Error context are tracked both locally (to a single component instance) and globally
        // the counts for both must stay in sync.
        //
        // Here we reflect the newly created global concurrent error context state into the
        // component instance's locally tracked count, along with the appropriate key into the global
        // ref tracking data structures to enable later lookup
        let local_tbl = self
            .error_context_tables()
            .get_mut_or_insert_with(ty, || StateTable::default());

        assert!(
            !local_tbl.has_handle(table_id.rep()),
            "newly created error context state already tracked by component"
        );
        let local_idx = local_tbl.insert(table_id.rep(), LocalErrorContextRefCount(1))?;

        Ok(local_idx)
    }

    pub(super) fn error_context_debug_message<T>(
        &mut self,
        store: StoreContextMut<T>,
        memory: *mut VMMemoryDefinition,
        realloc: *mut VMFuncRef,
        string_encoding: u8,
        ty: TypeComponentLocalErrorContextTableIndex,
        err_ctx_handle: u32,
        debug_msg_address: u32,
    ) -> Result<()> {
        let store_id = store.0.id();

        // Retrieve the error context and internal debug message
        let (state_table_id_rep, _) = self
            .error_context_tables()
            .get_mut(ty)
            .context("error context table index present in (sub)component lookup during debug_msg")?
            .get_mut_by_index(err_ctx_handle)?;

        // Get the state associated with the error context
        let ErrorContextState { debug_msg } =
            self.get_mut(TableId::<ErrorContextState>::new(state_table_id_rep))?;
        let debug_msg = debug_msg.clone();

        // Lower the string into the component's memory
        let options = unsafe {
            Options::new(
                store_id,
                NonNull::new(memory),
                NonNull::new(realloc),
                StringEncoding::from_u8(string_encoding).ok_or_else(|| {
                    anyhow::anyhow!("failed to convert u8 string encoding [{string_encoding}]")
                })?,
                false,
                None,
            )
        };
        let interface = self as *mut _;
        let lower_cx =
            unsafe { &mut LowerContext::new(store, &options, self.component_types(), interface) };
        let debug_msg_address = usize::try_from(debug_msg_address)?;
        let offset = lower_cx
            .as_slice_mut()
            .get(debug_msg_address..)
            .and_then(|b| b.get(..debug_msg.bytes().len()))
            .map(|_| debug_msg_address)
            .ok_or_else(|| anyhow::anyhow!("invalid debug message pointer: out of bounds"))?;
        debug_msg
            .as_str()
            .store(lower_cx, InterfaceType::String, offset)?;

        Ok(())
    }

    pub(crate) fn error_context_drop(
        &mut self,
        ty: TypeComponentLocalErrorContextTableIndex,
        error_context: u32,
    ) -> Result<()> {
        let local_state_table = self
            .error_context_tables()
            .get_mut(ty)
            .context("error context table index present in (sub)component table during drop")?;

        // Reduce the local (sub)component ref count, removing tracking if necessary
        let (rep, local_ref_removed) = {
            let (rep, LocalErrorContextRefCount(local_ref_count)) =
                local_state_table.get_mut_by_index(error_context)?;
            assert!(*local_ref_count > 0);
            *local_ref_count -= 1;
            let mut local_ref_removed = false;
            if *local_ref_count == 0 {
                local_ref_removed = true;
                local_state_table
                    .remove_by_index(error_context)
                    .context("removing error context from component-local tracking")?;
            }
            (rep, local_ref_removed)
        };

        let global_ref_count_idx = TypeComponentGlobalErrorContextTableIndex::from_u32(rep);

        let GlobalErrorContextRefCount(global_ref_count) = self
            .global_error_context_ref_counts()
            .get_mut(&global_ref_count_idx)
            .expect("retrieve concurrent state for error context during drop");

        // Reduce the component-global ref count, removing tracking if necessary
        assert!(*global_ref_count >= 1);
        *global_ref_count -= 1;
        if *global_ref_count == 0 {
            assert!(local_ref_removed);

            self.global_error_context_ref_counts()
                .remove(&global_ref_count_idx);

            self.delete(TableId::<ErrorContextState>::new(rep))
                .context("deleting component-global error context data")?;
        }

        Ok(())
    }

    fn guest_transfer<U: PartialEq + Eq + std::fmt::Debug>(
        &mut self,
        src_idx: u32,
        src: U,
        src_instance: RuntimeComponentInstanceIndex,
        dst: U,
        dst_instance: RuntimeComponentInstanceIndex,
        match_state: impl Fn(&mut WaitableState) -> Result<(U, &mut StreamFutureState)>,
        make_state: impl Fn(U, StreamFutureState) -> WaitableState,
    ) -> Result<u32> {
        let src_table = &mut self.waitable_tables()[src_instance];
        let (rep, src_state) = src_table.get_mut_by_index(src_idx)?;
        let (src_ty, _) = match_state(src_state)?;
        if src_ty != src {
            bail!("invalid future handle");
        }

        let state = self.get(TableId::<TransmitHandle>::new(rep))?.state;
        let state = self.get_mut(state)?;
        let read_handle = state.read_handle.rep();
        let write_handle = state.write_handle.rep();

        let src_table = &mut self.waitable_tables()[src_instance];
        let (rep, src_state) = src_table.get_mut_by_index(src_idx)?;
        let (_, src_state) = match_state(src_state)?;

        match src_state {
            StreamFutureState::Local => {
                *src_state = StreamFutureState::Write;
                let dst_table = &mut self.waitable_tables()[dst_instance];
                assert!(dst_table.get_mut_by_rep(read_handle).is_none());
                dst_table.insert(read_handle, make_state(dst, StreamFutureState::Read))
            }
            StreamFutureState::Read => {
                src_table.remove_by_index(src_idx)?;

                let dst_table = &mut self.waitable_tables()[dst_instance];
                if let Some((dst_idx, dst_state)) = dst_table.get_mut_by_rep(write_handle) {
                    let (dst_ty, dst_state) = match_state(dst_state).unwrap();
                    assert_eq!(dst_ty, dst);
                    assert_eq!(*dst_state, StreamFutureState::Write);
                    *dst_state = StreamFutureState::Local;
                    Ok(dst_idx)
                } else {
                    dst_table.insert(rep, make_state(dst, StreamFutureState::Read))
                }
            }
            StreamFutureState::Write => bail!("cannot transfer write end of stream or future"),
            StreamFutureState::Busy => bail!("cannot transfer busy stream or future"),
        }
    }

    pub(crate) fn future_new(&mut self, ty: TypeFutureTableIndex) -> Result<u32> {
        self.guest_new(TableIndex::Future(ty))
    }

    pub(crate) fn future_cancel_write(
        &mut self,
        ty: TypeFutureTableIndex,
        async_: bool,
        writer: u32,
    ) -> Result<u32> {
        self.guest_cancel_write(TableIndex::Future(ty), writer, async_)
    }

    pub(crate) fn future_cancel_read(
        &mut self,
        ty: TypeFutureTableIndex,
        async_: bool,
        reader: u32,
    ) -> Result<u32> {
        self.guest_cancel_read(TableIndex::Future(ty), reader, async_)
    }

    pub(crate) fn future_close_writable(
        &mut self,
        ty: TypeFutureTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        error: u32,
    ) -> Result<()> {
        self.guest_close_writable(TableIndex::Future(ty), err_ctx_ty, writer, error)
    }

    pub(crate) fn future_close_readable(
        &mut self,
        ty: TypeFutureTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        reader: u32,
        error: u32,
    ) -> Result<()> {
        self.guest_close_readable(TableIndex::Future(ty), err_ctx_ty, reader, error)
    }

    pub(crate) fn stream_new(&mut self, ty: TypeStreamTableIndex) -> Result<u32> {
        self.guest_new(TableIndex::Stream(ty))
    }

    pub(crate) fn stream_cancel_write(
        &mut self,
        ty: TypeStreamTableIndex,
        async_: bool,
        writer: u32,
    ) -> Result<u32> {
        self.guest_cancel_write(TableIndex::Stream(ty), writer, async_)
    }

    pub(crate) fn stream_cancel_read(
        &mut self,
        ty: TypeStreamTableIndex,
        async_: bool,
        reader: u32,
    ) -> Result<u32> {
        self.guest_cancel_read(TableIndex::Stream(ty), reader, async_)
    }

    pub(crate) fn stream_close_writable(
        &mut self,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        writer: u32,
        error: u32,
    ) -> Result<()> {
        self.guest_close_writable(TableIndex::Stream(ty), err_ctx_ty, writer, error)
    }

    pub(crate) fn stream_close_readable(
        &mut self,
        ty: TypeStreamTableIndex,
        err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
        reader: u32,
        error: u32,
    ) -> Result<()> {
        self.guest_close_readable(TableIndex::Stream(ty), err_ctx_ty, reader, error)
    }

    pub(crate) fn future_transfer(
        &mut self,
        src_idx: u32,
        src: TypeFutureTableIndex,
        dst: TypeFutureTableIndex,
    ) -> Result<u32> {
        self.guest_transfer(
            src_idx,
            src,
            self.component_types()[src].instance,
            dst,
            self.component_types()[dst].instance,
            |state| {
                if let WaitableState::Future(ty, state) = state {
                    Ok((*ty, state))
                } else {
                    Err(anyhow!("invalid future handle"))
                }
            },
            WaitableState::Future,
        )
    }

    pub(crate) fn stream_transfer(
        &mut self,
        src_idx: u32,
        src: TypeStreamTableIndex,
        dst: TypeStreamTableIndex,
    ) -> Result<u32> {
        self.guest_transfer(
            src_idx,
            src,
            self.component_types()[src].instance,
            dst,
            self.component_types()[dst].instance,
            |state| {
                if let WaitableState::Stream(ty, state) = state {
                    Ok((*ty, state))
                } else {
                    Err(anyhow!("invalid stream handle"))
                }
            },
            WaitableState::Stream,
        )
    }

    /// Transfer the state of a given error context from one component to another
    pub(crate) fn error_context_transfer(
        &mut self,
        src_idx: u32,
        src: TypeComponentLocalErrorContextTableIndex,
        dst: TypeComponentLocalErrorContextTableIndex,
    ) -> Result<u32> {
        let (rep, _) = {
            let rep = self
                .error_context_tables()
                .get_mut(src)
                .context("error context table index present in (sub)component lookup")?
                .get_mut_by_index(src_idx)?;
            rep
        };
        let dst = self
            .error_context_tables()
            .get_mut(dst)
            .context("error context table index present in (sub)component lookup")?;

        // Update the component local for the destination
        let updated_count = if let Some((dst_idx, count)) = dst.get_mut_by_rep(rep) {
            (*count).0 += 1;
            dst_idx
        } else {
            dst.insert(rep, LocalErrorContextRefCount(1))?
        };

        // Update the global (cross-subcomponent) count for error contexts
        // as the new component has essentially created a new reference that will
        // be dropped/handled independently
        let global_ref_count = self
            .global_error_context_ref_counts()
            .get_mut(&TypeComponentGlobalErrorContextTableIndex::from_u32(rep))
            .context("global ref count present for existing (sub)component error context")?;
        global_ref_count.0 += 1;

        Ok(updated_count)
    }
}
