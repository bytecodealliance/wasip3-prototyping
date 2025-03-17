use {
    super::{
        table::TableId, Event, GuestTask, HostTaskFuture, HostTaskOutput, HostTaskResult, Promise,
    },
    crate::{
        component::{
            func::{self, Lift, LiftContext, LowerContext, Options},
            matching::InstanceType,
            values::{ErrorContextAny, FutureAny, StreamAny},
            Lower, Val, WasmList, WasmStr,
        },
        vm::{
            component::{
                ComponentInstance, ErrorContextState, GlobalErrorContextRefCount,
                LocalErrorContextRefCount, StateTable, StreamFutureState, WaitableState,
            },
            SendSyncPtr, VMFuncRef, VMMemoryDefinition, VMStore,
        },
        AsContextMut, StoreContextMut, VMStoreRawPtr, ValRaw,
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
        ops::Deref,
        pin::Pin,
        ptr::NonNull,
        string::ToString,
        sync::Arc,
        task::Poll,
        vec::Vec,
    },
    wasmtime_environ::component::{
        CanonicalAbiInfo, ComponentTypes, InterfaceType, StringEncoding,
        TypeComponentGlobalErrorContextTableIndex, TypeComponentLocalErrorContextTableIndex,
        TypeFutureTableIndex, TypeStreamTableIndex,
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

fn state_table(instance: &mut ComponentInstance, ty: TableIndex) -> &mut StateTable<WaitableState> {
    let runtime_instance = match ty {
        TableIndex::Stream(ty) => instance.component_types()[ty].instance,
        TableIndex::Future(ty) => instance.component_types()[ty].instance,
    };
    &mut instance.component_waitable_tables()[runtime_instance]
}

fn push_event<T>(
    mut store: StoreContextMut<T>,
    rep: u32,
    event: Event,
    param: usize,
    caller: TableId<GuestTask>,
) {
    store
        .concurrent_state()
        .futures
        .get_mut()
        .push(Box::pin(future::ready(HostTaskOutput::Call {
            waitable: rep,
            fun: Box::new(move |_| {
                Ok(HostTaskResult {
                    event,
                    param: u32::try_from(param).unwrap(),
                    caller,
                })
            }),
        })) as HostTaskFuture);
}

fn get_mut_by_index(
    instance: &mut ComponentInstance,
    ty: TableIndex,
    index: u32,
) -> Result<(u32, &mut StreamFutureState)> {
    get_mut_by_index_from(state_table(instance, ty), ty, index)
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
    mut tx: mpsc::Sender<Option<TypeComponentGlobalErrorContextTableIndex>>,
) -> impl FnOnce(Reader) -> Result<usize> + Send + Sync + 'static {
    move |reader| {
        let count = match reader {
            Reader::Guest {
                lower:
                    RawLowerContext {
                        store,
                        options,
                        types,
                        instance,
                    },
                ty,
                address,
                count,
            } => {
                let mut store = unsafe { StoreContextMut::<U>(&mut *store.cast()) };
                let lower = &mut unsafe {
                    LowerContext::new(store.as_context_mut(), options, types, instance)
                };
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
                    let transmit = store.concurrent_state().table.get_mut(transmit_id)?;
                    assert!(matches!(&transmit.write, WriteState::Open));

                    transmit.write = WriteState::HostReady {
                        accept: Box::new(accept::<T, B, U>(values, offset, transmit_id, tx)),
                        post_write: PostWrite::Continue,
                    };
                } else {
                    _ = tx.try_send(None)?;
                }

                count
            }
            Reader::Host { accept } => {
                assert!(offset == 0); // todo: do we need to handle offset != 0?
                let count = values.len();
                accept(Box::new(values))?;
                _ = tx.try_send(None)?;
                count
            }
        };

        Ok(count)
    }
}

/// Write to a waitable from the host
///
/// # Arguments
///
/// * `store` - The engine store
/// * `transmit_rep` - Global representation of the transmit object that will be modified
/// * `values` - List of values that should be written
/// * `post_write` - Whether the transmit should be closed after write, possibly with an error context
///
/// # Returns
///
/// `host_write` can return an optional component-global error context table if the
/// reader end was in an error state when the write was performed. This context has to be
/// propagated by the caller of `host_write` to the relevant writer.
///
fn host_write<
    T: func::Lower + Send + Sync + 'static,
    B: Buffer<T>,
    U,
    S: AsContextMut<Data = U>,
>(
    mut store: S,
    transmit_rep: u32,
    values: B,
    mut post_write: PostWrite,
    tx: mpsc::Sender<Option<TypeComponentGlobalErrorContextTableIndex>>,
) -> Result<Option<TypeComponentGlobalErrorContextTableIndex>> {
    let mut store = store.as_context_mut();
    let transmit_id = TableId::<TransmitState>::new(transmit_rep);
    let mut offset = 0;

    let mut tx = Some(tx);

    loop {
        let transmit = store
            .concurrent_state()
            .table
            .get_mut(transmit_id)
            .with_context(|| format!("retrieving state for transmit [{transmit_rep}]"))?;

        let new_state = if let ReadState::Closed(err_ctx) = &transmit.read {
            ReadState::Closed(*err_ctx)
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
                instance,
                handle,
                caller,
                ..
            } => unsafe {
                let types = (*instance.as_ptr()).component_types();
                let lower = &mut LowerContext::new(
                    store.as_context_mut(),
                    &options,
                    types,
                    instance.as_ptr(),
                );
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

                log::trace!("remove read child of {}: {transmit_rep}", caller.rep());
                store
                    .concurrent_state()
                    .table
                    .remove_child(transmit_id, caller)?;

                *get_mut_by_index(&mut *instance.as_ptr(), ty, handle)?.1 = StreamFutureState::Read;

                push_event(
                    store.as_context_mut(),
                    transmit_rep,
                    match ty {
                        TableIndex::Future(_) => Event::FutureRead,
                        TableIndex::Stream(_) => Event::StreamRead,
                    },
                    count,
                    caller,
                );

                if offset < values.len() {
                    continue;
                }
            },

            ReadState::HostReady { accept } => {
                accept(Writer::Host {
                    values: Box::new(values),
                })?;
            }

            ReadState::Closed(err_ctx) => {
                if let Some(global_err_ctx_ty) = err_ctx {
                    break Ok(Some(global_err_ctx_ty));
                }

                store.concurrent_state().table.delete(transmit_id)?;
                break Ok(None);
            }
        }

        if let PostWrite::Close(err_ctx) = post_write {
            host_close_writer(store, transmit_rep, err_ctx)?;
        }

        if let Some(mut tx) = tx.take() {
            _ = tx.try_send(None)?;
        }

        break Ok(None);
    }
}

pub fn host_read<
    T: func::Lift + Sync + Send + 'static,
    B: Buffer<T>,
    U,
    S: AsContextMut<Data = U>,
>(
    mut store: S,
    rep: u32,
    tx: oneshot::Sender<HostReadResult<B>>,
) -> Result<()> {
    let mut store = store.as_context_mut();
    let transmit_id = TableId::<TransmitState>::new(rep);
    let transmit = store
        .concurrent_state()
        .table
        .get_mut(transmit_id)
        .with_context(|| rep.to_string())?;

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
                                _ = tx.send(HostReadResult::EndOfStream(Some(ErrorContext::new(
                                    err_ctx.as_u32(),
                                ))));
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
            instance,
            handle,
            caller,
            post_write,
            ..
        } => unsafe {
            let types = (*instance.as_ptr()).component_types();
            let lift = &mut LiftContext::new(store.0, &options, types, instance.as_ptr());
            _ = tx.send(HostReadResult::Values(if T::IS_RUST_UNIT_TYPE {
                B::from(
                    iter::repeat_with(|| MaybeUninit::uninit().assume_init())
                        .take(count)
                        .collect(),
                )
            } else {
                let ty = payload(ty, types).unwrap();
                let list = &WasmList::new(address, count, lift, ty)?;
                B::from(T::load_list(lift, list)?)
            }));

            log::trace!(
                "remove write child of {}: {}",
                caller.rep(),
                transmit_id.rep()
            );
            store
                .concurrent_state()
                .table
                .remove_child(transmit_id, caller)?;

            if let PostWrite::Close(err_ctx) = post_write {
                store.concurrent_state().table.get_mut(transmit_id)?.write =
                    WriteState::Closed(err_ctx);
            } else {
                *get_mut_by_index(&mut *instance.as_ptr(), ty, handle)?.1 =
                    StreamFutureState::Write;
            }

            push_event(
                store,
                transmit_id.rep(),
                match ty {
                    TableIndex::Future(_) => Event::FutureWrite,
                    TableIndex::Stream(_) => Event::StreamWrite,
                },
                count,
                caller,
            );
        },

        WriteState::HostReady { accept, post_write } => {
            accept(Reader::Host {
                accept: Box::new(move |any| {
                    _ = tx.send(HostReadResult::Values(
                        *any.downcast()
                            .map_err(|_| anyhow!("transmit type mismatch"))?,
                    ));
                    Ok(())
                }),
            })?;

            if let PostWrite::Close(err_ctx) = post_write {
                store.concurrent_state().table.get_mut(transmit_id)?.write =
                    WriteState::Closed(err_ctx);
            }
        }

        WriteState::Closed(err_ctx) => {
            host_close_reader(store, rep, err_ctx.map(|v| v.as_u32()))?;
        }
    }

    Ok(())
}

fn host_cancel_write<U, S: AsContextMut<Data = U>>(mut store: S, rep: u32) -> Result<u32> {
    let mut store = store.as_context_mut();
    let transmit_id = TableId::<TransmitState>::new(rep);
    let transmit = store.concurrent_state().table.get_mut(transmit_id)?;

    match &transmit.write {
        WriteState::GuestReady { caller, .. } => {
            let caller = *caller;
            transmit.write = WriteState::Open;
            store
                .concurrent_state()
                .table
                .remove_child(transmit_id, caller)?;
        }

        WriteState::HostReady { .. } => {
            transmit.write = WriteState::Open;
        }

        WriteState::Open | WriteState::Closed(_) => {
            bail!("stream or future write canceled when no write is pending")
        }
    }

    log::trace!("canceled write {rep}");

    Ok(0)
}

fn host_cancel_read<U, S: AsContextMut<Data = U>>(mut store: S, rep: u32) -> Result<u32> {
    let mut store = store.as_context_mut();
    let transmit_id = TableId::<TransmitState>::new(rep);
    let transmit = store.concurrent_state().table.get_mut(transmit_id)?;

    match &transmit.read {
        ReadState::GuestReady { caller, .. } => {
            let caller = *caller;
            transmit.read = ReadState::Open;
            store
                .concurrent_state()
                .table
                .remove_child(transmit_id, caller)?;
        }

        ReadState::HostReady { .. } => {
            transmit.read = ReadState::Open;
        }

        ReadState::Open | ReadState::Closed(_) => {
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
/// * `store` - the store for the component
/// * `transmit_rep` - A component-global representation of the transmit state for the writer that should be closed
/// * `err_ctx` - An optional component-global representation of an error context to use as the final value of the writer
///
fn host_close_writer<U, S: AsContextMut<Data = U>>(
    mut store: S,
    transmit_rep: u32,
    err_ctx: Option<TypeComponentGlobalErrorContextTableIndex>,
) -> Result<()> {
    let mut store = store.as_context_mut();
    let transmit_id = TableId::<TransmitState>::new(transmit_rep);
    let transmit = store
        .concurrent_state()
        .table
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
    let new_state = if let ReadState::Closed(err_ctx) = &transmit.read {
        ReadState::Closed(*err_ctx)
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
            instance,
            handle,
            caller,
            ..
        } => unsafe {
            // Lift the global err_ctx that we're receiving into an error context
            // reference that the reader(caller) will
            let reader_state_tbl = (*instance.as_ptr())
                .component_error_context_tables()
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
            push_event(
                store,
                transmit_id.rep(),
                match ty {
                    TableIndex::Future(_) => Event::FutureRead,
                    TableIndex::Stream(_) => Event::StreamRead,
                },
                push_param,
                caller,
            );

            *get_mut_by_index(&mut *instance.as_ptr(), ty, handle)?.1 = StreamFutureState::Read;
        },

        // If the host was ready to read, and the writer end is being closed (host->host write?)
        // signal to the reader that we've reached the end of the stream, and close the reader immediately
        ReadState::HostReady { accept } => {
            accept(Writer::End(err_ctx))?;
            host_close_reader(store, transmit_rep, None)?;
        }

        // If the read state is open, then there are no registered readers of the stream/future
        ReadState::Open => {}

        // If the read state was already closed, then we can remove the transmit state completely
        // (both writer and reader have been closed)
        ReadState::Closed(_err_ctx) => {
            log::trace!("host_close_writer delete {transmit_rep}");
            store.concurrent_state().table.delete(transmit_id)?;
        }
    }
    Ok(())
}

/// Close the reader end of a Future or Stream
///
/// # Arguments
///
/// * `store` - The store for the component
/// * `transmit_rep` - A global-component-level representation of the transmit state for the reader that should be closed
/// * `err_ctx` - A global-component-level representation of an error context to attach, if any
///
fn host_close_reader<U, S: AsContextMut<Data = U>>(
    mut store: S,
    transmit_rep: u32,
    err_ctx: Option<u32>,
) -> Result<()> {
    let mut store = store.as_context_mut();
    let transmit_id = TableId::<TransmitState>::new(transmit_rep);
    let transmit = store
        .concurrent_state()
        .table
        .get_mut(transmit_id)
        .with_context(|| format!("reader {transmit_rep}"))?;

    transmit.read =
        ReadState::Closed(err_ctx.map(TypeComponentGlobalErrorContextTableIndex::from_u32));
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
            instance,
            handle,
            post_write,
            caller,
            ..
        } => unsafe {
            push_event(
                store.as_context_mut(),
                transmit_id.rep(),
                match ty {
                    TableIndex::Future(_) => Event::FutureRead,
                    TableIndex::Stream(_) => Event::StreamRead,
                },
                CLOSED,
                caller,
            );

            if let PostWrite::Close(_) = post_write {
                store.concurrent_state().table.delete(transmit_id)?;
            } else {
                *get_mut_by_index(&mut *instance.as_ptr(), ty, handle)?.1 =
                    StreamFutureState::Write;
            }
        },

        // If the reader is closed, we can ignore the waiting write from  host
        WriteState::HostReady { .. } => {
            store.concurrent_state().table.delete(transmit_id)?;
        }

        WriteState::Open => {}

        WriteState::Closed(_) => {
            log::trace!("host_close_reader delete {transmit_rep}");
            store.concurrent_state().table.delete(transmit_id)?;
        }
    }
    Ok(())
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
        tx: mpsc::Sender<Option<TypeComponentGlobalErrorContextTableIndex>>,
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
    WatchReader {
        tx: oneshot::Sender<()>,
    },
    CloseReader {
        err_ctx: Option<u32>,
    },
}

fn start_event_loop<
    T: func::Lower + func::Lift + Sync + Send + 'static,
    B: Buffer<T>,
    U,
    S: AsContextMut<Data = U>,
>(
    mut store: S,
    rep: u32,
) -> mpsc::Sender<StreamOrFutureEvent<B>> {
    fn write<T: func::Lower + Sync + Send + 'static, B: Buffer<T>, U>(
        store: VMStoreRawPtr,
        rep: u32,
        values: B,
        post_write: PostWrite,
        tx: mpsc::Sender<Option<TypeComponentGlobalErrorContextTableIndex>>,
    ) -> Result<Option<TypeComponentGlobalErrorContextTableIndex>> {
        let store = unsafe { StoreContextMut::<U>(&mut *store.0.as_ptr().cast()) };
        host_write(store, rep, values, post_write, tx)
    }

    fn close_writer<U>(
        store: VMStoreRawPtr,
        rep: u32,
        err_ctx: Option<ErrorContext>,
    ) -> Result<()> {
        let store = unsafe { StoreContextMut::<U>(&mut *store.0.as_ptr().cast()) };
        host_close_writer(
            store,
            rep,
            err_ctx.map(|v| TypeComponentGlobalErrorContextTableIndex::from_u32(v.rep)),
        )
    }

    fn watch_writer<U>(store: VMStoreRawPtr, rep: u32, tx: oneshot::Sender<()>) -> Result<()> {
        let mut store = unsafe { StoreContextMut::<U>(&mut *store.0.as_ptr().cast()) };
        let state = store
            .concurrent_state()
            .table
            .get_mut(TableId::<TransmitState>::new(rep))?;
        if !matches!(&state.write, WriteState::Closed(_)) {
            state.writer_watcher = Some(tx);
        }
        Ok(())
    }

    fn read<T: func::Lift + Sync + Send + 'static, B: Buffer<T>, U>(
        store: VMStoreRawPtr,
        rep: u32,
        tx: oneshot::Sender<HostReadResult<B>>,
    ) -> Result<()> {
        let store = unsafe { StoreContextMut::<U>(&mut *store.0.as_ptr().cast()) };
        host_read::<T, B, _, _>(store, rep, tx)
    }

    fn close_reader<U>(store: VMStoreRawPtr, rep: u32, err_ctx: Option<u32>) -> Result<()> {
        let store = unsafe { StoreContextMut::<U>(&mut *store.0.as_ptr().cast()) };
        host_close_reader(store, rep, err_ctx)
    }

    fn watch_reader<U>(store: VMStoreRawPtr, rep: u32, tx: oneshot::Sender<()>) -> Result<()> {
        let mut store = unsafe { StoreContextMut::<U>(&mut *store.0.as_ptr().cast()) };
        let state = store
            .concurrent_state()
            .table
            .get_mut(TableId::<TransmitState>::new(rep))?;
        if !matches!(&state.read, ReadState::Closed { .. }) {
            state.reader_watcher = Some(tx);
        }
        Ok(())
    }

    let mut store = store.as_context_mut();
    let (tx, mut rx) = mpsc::channel(1);
    let task = Box::pin(
        {
            let store = VMStoreRawPtr(store.0.traitobj());
            async move {
                while let Some(event) = rx.next().await {
                    match event {
                        StreamOrFutureEvent::Write {
                            values,
                            post_write,
                            mut tx,
                        } => match write::<T, B, U>(store, rep, values, post_write, tx.clone())? {
                            // A host write will return an global-component error context when it encounters a closed
                            // reader. In that case, we must bubble the error context that was reported by the reader
                            // back up to the caller of this write, so that the error can be returned.
                            //
                            // The tx will not be used, so we can use it to send our error value
                            //
                            Some(err_ctx) => {
                                tx.try_send(Some(err_ctx))?;
                                ()
                            }
                            None => (),
                        },
                        StreamOrFutureEvent::CloseWriter { err_ctx } => {
                            close_writer::<U>(store, rep, err_ctx)?
                        }
                        StreamOrFutureEvent::WatchWriter { tx } => {
                            watch_writer::<U>(store, rep, tx)?
                        }
                        StreamOrFutureEvent::Read { tx } => read::<T, B, U>(store, rep, tx)?,
                        StreamOrFutureEvent::CloseReader { err_ctx } => {
                            close_reader::<U>(store, rep, err_ctx)?
                        }
                        StreamOrFutureEvent::WatchReader { tx } => {
                            watch_reader::<U>(store, rep, tx)?
                        }
                    }
                }
                Ok(())
            }
        }
        .map(HostTaskOutput::Background),
    );
    store.concurrent_state().futures.get_mut().push(task);
    tx
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

/// Wrapper `struct` that implements `Future` to represent a state change
/// concerning the inner value.
///
/// This `Future` will resolve when the state change occurs, and may be
/// converted back into the inner value using `into_inner` before or after that
/// happens.
pub struct Watch<T> {
    inner: T,
    rx: oneshot::Receiver<()>,
}

impl<T: Ready> Watch<T> {
    /// Convert this object into its inner value.
    pub async fn into_inner(mut self) -> T {
        self.inner.ready().await;
        self.inner
    }
}

impl<T: Unpin> Future for Watch<T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        self.get_mut().rx.poll_unpin(cx).map(drop)
    }
}

/// Represents the writable end of a Component Model `future`.
pub struct FutureWriter<T> {
    tx: Option<mpsc::Sender<StreamOrFutureEvent<Single<T>>>>,
}

impl<T> FutureWriter<T> {
    /// Write the specified value to this `future`.
    ///
    /// The returned `Promise` will yield `true` if the read end accepted the
    /// value; otherwise it will return `false`, meaning the read end was closed
    /// before the value could be delivered.
    pub fn write(mut self, value: T) -> Promise<Result<(), ErrorContext>> {
        let (tx, rx) = mpsc::channel::<Option<TypeComponentGlobalErrorContextTableIndex>>(1);
        send(
            &mut self.tx.take().unwrap(),
            StreamOrFutureEvent::Write {
                values: Single(value),
                post_write: PostWrite::Close(None),
                tx,
            },
        );
        Promise(Box::pin(async {
            match StreamExt::into_future(rx).await.0 {
                None => unreachable!(),
                Some(Some(rep)) => Err(ErrorContext::new(rep.as_u32())),
                Some(None) => Ok(()),
            }
        }))
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

    /// Convert this object into a `Watch` which will resolve when the write end
    /// of this `stream` is closed.
    ///
    /// You may convert the `Watch` back into a `FutureWriter` at any time using
    /// `Watch::into_inner`.
    pub fn watch_reader(mut self) -> Watch<Self> {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchReader { tx },
        );
        Watch { inner: self, rx }
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
    rep: u32,
    _phantom: PhantomData<T>,
}

impl<T> HostFuture<T> {
    pub(crate) fn new(rep: u32) -> Self {
        Self {
            rep,
            _phantom: PhantomData,
        }
    }

    /// Convert this object into a [`FutureReader`].
    pub fn into_reader<U, S: AsContextMut<Data = U>>(self, store: S) -> FutureReader<T>
    where
        T: func::Lower + func::Lift + Send + Sync + 'static,
    {
        FutureReader {
            rep: self.rep,
            tx: Some(start_event_loop(store, self.rep)),
        }
    }

    /// Convert this `FutureReader` into a [`Val`].
    // See TODO comment for `FutureAny`; this is prone to handle leakage.
    pub fn into_val(self) -> Val {
        Val::Future(FutureAny(self.rep))
    }

    /// Attempt to convert the specified [`Val`] to a `FutureReader`.
    pub fn from_val<U, S: AsContextMut<Data = U>>(mut store: S, value: &Val) -> Result<Self> {
        let Val::Future(FutureAny(rep)) = value else {
            bail!("expected `future`; got `{}`", value.desc());
        };
        store
            .as_context_mut()
            .concurrent_state()
            .table
            .get(TableId::<TransmitState>::new(*rep))?;
        Ok(Self::new(*rep))
    }

    fn lower_to_index<U>(&self, cx: &mut LowerContext<'_, U>, ty: InterfaceType) -> Result<u32> {
        match ty {
            InterfaceType::Future(dst) => {
                state_table(unsafe { &mut *cx.instance }, TableIndex::Future(dst)).insert(
                    self.rep,
                    WaitableState::Future(dst, StreamFutureState::Read),
                )
            }
            _ => func::bad_type_info(),
        }
    }

    fn lift_from_index(cx: &mut LiftContext<'_>, ty: InterfaceType, index: u32) -> Result<Self> {
        match ty {
            InterfaceType::Future(src) => {
                let state_table =
                    state_table(unsafe { &mut *cx.instance }, TableIndex::Future(src));
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

                Ok(Self {
                    rep,
                    _phantom: PhantomData,
                })
            }
            _ => func::bad_type_info(),
        }
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
        self.lower_to_index(cx, ty)?
            .lower(cx, InterfaceType::U32, dst)
    }

    fn store<U>(
        &self,
        cx: &mut LowerContext<'_, U>,
        ty: InterfaceType,
        offset: usize,
    ) -> Result<()> {
        self.lower_to_index(cx, ty)?
            .store(cx, InterfaceType::U32, offset)
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
    rep: u32,
    tx: Option<mpsc::Sender<StreamOrFutureEvent<Single<T>>>>,
}

impl<T> FutureReader<T> {
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
        Promise(Box::pin(rx.map(|v| match v {
            Ok(HostReadResult::Values(v)) => Ok(v.0),
            Ok(HostReadResult::EndOfStream(None)) => unreachable!(),
            Err(_) => Err(None),
            Ok(HostReadResult::EndOfStream(s)) => Err(s),
        })))
    }

    /// Convert this object into a `Watch` which will resolve when the write end
    /// of this `stream` is closed.
    ///
    /// You may convert the `Watch` back into a `FutureReader` at any time using
    /// `Watch::into_inner`.
    pub fn watch_writer(mut self) -> Watch<Self> {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchWriter { tx },
        );
        Watch { inner: self, rx }
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
            send(&mut tx, StreamOrFutureEvent::CloseReader { err_ctx: None });
        }
    }
}

/// Create a new Component Model `future` as pair of writable and readable ends,
/// the latter of which may be passed to guest code.
pub fn future<T: func::Lower + func::Lift + Sync + Send + 'static, U, S: AsContextMut<Data = U>>(
    mut store: S,
) -> Result<(FutureWriter<T>, FutureReader<T>)> {
    let mut store = store.as_context_mut();
    let transmit = store.concurrent_state().table.push(TransmitState {
        read: ReadState::Open,
        write: WriteState::Open,
        reader_watcher: None,
        writer_watcher: None,
    })?;

    Ok((
        FutureWriter {
            tx: Some(start_event_loop(&mut store, transmit.rep())),
        },
        FutureReader {
            rep: transmit.rep(),
            tx: Some(start_event_loop(store, transmit.rep())),
        },
    ))
}

/// Represents the writable end of a Component Model `stream`.
pub struct StreamWriter<B> {
    tx: Option<mpsc::Sender<StreamOrFutureEvent<B>>>,
}

impl<B> StreamWriter<B> {
    /// Write the specified values to the `stream`.
    ///
    /// The returned `Promise` will yield `None` if the read end has been closed
    /// before it could accept all of the values; otherwise it will yield
    /// `self`.
    pub fn write(mut self, values: B) -> Promise<Result<StreamWriter<B>, ErrorContext>>
    where
        B: Send + 'static,
    {
        let (tx, rx) = mpsc::channel(1);
        send(
            self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::Write {
                values,
                post_write: PostWrite::Continue,
                tx,
            },
        );
        Promise(Box::pin(async {
            match StreamExt::into_future(rx).await.0 {
                None => unreachable!(),
                Some(None) => Ok(self),
                Some(Some(err_ctx)) => {
                    self.tx.take();
                    Err(ErrorContext::new(err_ctx.as_u32()))
                }
            }
        }))
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

    /// Convert this object into a `Watch` which will resolve when the read end
    /// of this `stream` is closed.
    ///
    /// You may convert the `Watch` back into a `StreamWriter` at any time using
    /// `Watch::into_inner`.
    pub fn watch_reader(mut self) -> Watch<Self> {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchReader { tx },
        );
        Watch { inner: self, rx }
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
    rep: u32,
    _phantom: PhantomData<T>,
}

impl<T> HostStream<T> {
    pub(crate) fn new(rep: u32) -> Self {
        Self {
            rep,
            _phantom: PhantomData,
        }
    }

    /// Convert this object into a [`StreamReader`].
    pub fn into_reader<B, U, S: AsContextMut<Data = U>>(self, store: S) -> StreamReader<B>
    where
        T: func::Lower + func::Lift + Sync + Send + 'static,
        B: Buffer<T>,
    {
        StreamReader {
            rep: self.rep,
            tx: Some(start_event_loop(store, self.rep)),
        }
    }

    /// Convert this `HostStream` into a [`Val`].
    // See TODO comment for `StreamAny`; this is prone to handle leakage.
    pub fn into_val(self) -> Val {
        Val::Stream(StreamAny(self.rep))
    }

    /// Attempt to convert the specified [`Val`] to a `HostStream`.
    pub fn from_val<U, S: AsContextMut<Data = U>>(mut store: S, value: &Val) -> Result<Self> {
        let Val::Stream(StreamAny(rep)) = value else {
            bail!("expected `stream`; got `{}`", value.desc());
        };
        store
            .as_context_mut()
            .concurrent_state()
            .table
            .get(TableId::<TransmitState>::new(*rep))?;
        Ok(Self::new(*rep))
    }

    fn lower_to_index<U>(&self, cx: &mut LowerContext<'_, U>, ty: InterfaceType) -> Result<u32> {
        match ty {
            InterfaceType::Stream(dst) => {
                state_table(unsafe { &mut *cx.instance }, TableIndex::Stream(dst)).insert(
                    self.rep,
                    WaitableState::Stream(dst, StreamFutureState::Read),
                )
            }
            _ => func::bad_type_info(),
        }
    }

    fn lift_from_index(cx: &mut LiftContext<'_>, ty: InterfaceType, index: u32) -> Result<Self> {
        match ty {
            InterfaceType::Stream(src) => {
                let state_table =
                    state_table(unsafe { &mut *cx.instance }, TableIndex::Stream(src));
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

                Ok(Self {
                    rep,
                    _phantom: PhantomData,
                })
            }
            _ => func::bad_type_info(),
        }
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
        self.lower_to_index(cx, ty)?
            .lower(cx, InterfaceType::U32, dst)
    }

    fn store<U>(
        &self,
        cx: &mut LowerContext<'_, U>,
        ty: InterfaceType,
        offset: usize,
    ) -> Result<()> {
        self.lower_to_index(cx, ty)?
            .store(cx, InterfaceType::U32, offset)
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
    rep: u32,
    tx: Option<mpsc::Sender<StreamOrFutureEvent<B>>>,
}

impl<B> StreamReader<B> {
    /// Read the value from this `stream`.
    pub fn read(mut self) -> Promise<Result<(StreamReader<B>, B), Option<ErrorContext>>>
    where
        B: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        send(self.tx.as_mut().unwrap(), StreamOrFutureEvent::Read { tx });
        Promise(Box::pin(rx.map(|v| match v {
            Ok(HostReadResult::Values(v)) => Ok((self, v)),
            Err(_) => {
                self.tx.take();
                Err(None)
            }
            Ok(HostReadResult::EndOfStream(s)) => {
                self.tx.take();
                Err(s)
            }
        })))
    }

    /// Convert this object into a `Watch` which will resolve when the write end
    /// of this `stream` is closed.
    ///
    /// You may convert the `Watch` back into a `StreamReader` at any time using
    /// `Watch::into_inner`.
    pub fn watch_writer(mut self) -> Watch<Self> {
        let (tx, rx) = oneshot::channel();
        send(
            &mut self.tx.as_mut().unwrap(),
            StreamOrFutureEvent::WatchWriter { tx },
        );
        Watch { inner: self, rx }
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
            send(&mut tx, StreamOrFutureEvent::CloseReader { err_ctx: None });
        }
    }
}

/// Create a new Component Model `stream` as pair of writable and readable ends,
/// the latter of which may be passed to guest code.
pub fn stream<
    T: func::Lower + func::Lift + Sync + Send + 'static,
    B: Buffer<T>,
    U,
    S: AsContextMut<Data = U>,
>(
    mut store: S,
) -> Result<(StreamWriter<B>, StreamReader<B>)> {
    let mut store = store.as_context_mut();
    let transmit = store.concurrent_state().table.push(TransmitState {
        read: ReadState::Open,
        write: WriteState::Open,
        reader_watcher: None,
        writer_watcher: None,
    })?;

    Ok((
        StreamWriter {
            tx: Some(start_event_loop(&mut store, transmit.rep())),
        },
        StreamReader {
            rep: transmit.rep(),
            tx: Some(start_event_loop(store, transmit.rep())),
        },
    ))
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

    fn lower_to_index<U>(&self, cx: &mut LowerContext<'_, U>, ty: InterfaceType) -> Result<u32> {
        match ty {
            InterfaceType::ErrorContext(dst) => {
                let tbl = unsafe {
                    &mut (*cx.instance)
                        .component_error_context_tables()
                        .get_mut(dst)
                        .expect("error context table index present in (sub)component table during lower")
                };

                if let Some((dst_idx, dst_state)) = tbl.get_mut_by_rep(self.rep) {
                    dst_state.0 += 1;
                    Ok(dst_idx)
                } else {
                    tbl.insert(self.rep, LocalErrorContextRefCount(1))
                }
            }
            _ => func::bad_type_info(),
        }
    }

    fn lift_from_index(cx: &mut LiftContext<'_>, ty: InterfaceType, index: u32) -> Result<Self> {
        match ty {
            InterfaceType::ErrorContext(src) => {
                let (rep, _) = unsafe {
                    (*cx.instance)
                        .component_error_context_tables()
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
        self.lower_to_index(cx, ty)?
            .lower(cx, InterfaceType::U32, dst)
    }

    fn store<T>(
        &self,
        cx: &mut LowerContext<'_, T>,
        ty: InterfaceType,
        offset: usize,
    ) -> Result<()> {
        self.lower_to_index(cx, ty)?
            .store(cx, InterfaceType::U32, offset)
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

/// Create a new error context
pub fn error_context<U, S: AsContextMut<Data = U>>(
    mut store: S,
    debug_msg: &str,
) -> Result<ErrorContext> {
    let mut store = store.as_context_mut();
    let entry = store.concurrent_state().table.push(ErrorContextState {
        debug_msg: debug_msg.into(),
    })?;
    // TODO: increment the component-global count for error contexts?
    Ok(ErrorContext::new(entry.rep()))
}

pub(super) struct TransmitState {
    write: WriteState,
    read: ReadState,
    writer_watcher: Option<oneshot::Sender<()>>,
    reader_watcher: Option<oneshot::Sender<()>>,
}

enum WriteState {
    Open,
    GuestReady {
        ty: TableIndex,
        flat_abi: Option<FlatAbi>,
        options: Options,
        address: usize,
        count: usize,
        instance: SendSyncPtr<ComponentInstance>,
        handle: u32,
        caller: TableId<GuestTask>,
        post_write: PostWrite,
    },
    HostReady {
        accept: Box<dyn FnOnce(Reader) -> Result<usize> + Send + Sync>,
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
        instance: SendSyncPtr<ComponentInstance>,
        handle: u32,
        caller: TableId<GuestTask>,
    },
    HostReady {
        accept: Box<dyn FnOnce(Writer) -> Result<usize> + Send + Sync>,
    },
    Closed(Option<TypeComponentGlobalErrorContextTableIndex>),
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
    store: *mut dyn VMStore,
    options: &'a Options,
    types: &'a Arc<ComponentTypes>,
    instance: *mut ComponentInstance,
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

pub(super) fn guest_new<T>(
    mut cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
    ty: TableIndex,
) -> Result<u32> {
    let transmit = cx.concurrent_state().table.push(TransmitState {
        read: ReadState::Open,
        write: WriteState::Open,
        reader_watcher: None,
        writer_watcher: None,
    })?;
    state_table(instance, ty).insert(transmit.rep(), waitable_state(ty, StreamFutureState::Local))
}

fn copy<T>(
    mut cx: StoreContextMut<'_, T>,
    types: &Arc<ComponentTypes>,
    instance: *mut ComponentInstance,
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

            let val = types[types[write_ty].ty]
                .payload
                .map(|ty| {
                    let abi = types.canonical_abi(&ty);
                    // FIXME: needs to read an i64 for memory64
                    if write_address % usize::try_from(abi.align32)? != 0 {
                        bail!("write pointer not aligned");
                    }

                    let lift =
                        &mut unsafe { LiftContext::new(cx.0, write_options, types, instance) };

                    let bytes = lift
                        .memory()
                        .get(write_address..)
                        .and_then(|b| b.get(..usize::try_from(abi.size32).unwrap()))
                        .ok_or_else(|| anyhow::anyhow!("write pointer out of bounds of memory"))?;

                    Val::load(lift, ty, bytes)
                })
                .transpose()?;

            if let Some(val) = val {
                let mut lower = unsafe {
                    LowerContext::new(cx.as_context_mut(), read_options, types, instance)
                };
                let ty = types[types[read_ty].ty].payload.unwrap();
                let ptr = func::validate_inbounds_dynamic(
                    types.canonical_abi(&ty),
                    lower.as_slice_mut(),
                    &ValRaw::u32(read_address.try_into().unwrap()),
                )?;
                val.store(&mut lower, ty, ptr)?;
            }
        }
        (TableIndex::Stream(write_ty), TableIndex::Stream(read_ty)) => {
            let lift = &mut unsafe { LiftContext::new(cx.0, write_options, types, instance) };
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
                            .memory(cx.0)
                            .get(write_address..)
                            .and_then(|b| b.get(..length_in_bytes))
                            .ok_or_else(|| {
                                anyhow::anyhow!("write pointer out of bounds of memory")
                            })?
                            .as_ptr();
                        let dst = read_options
                            .memory_mut(cx.0)
                            .get_mut(read_address..)
                            .and_then(|b| b.get_mut(..length_in_bytes))
                            .ok_or_else(|| anyhow::anyhow!("read pointer out of bounds of memory"))?
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

                let lower = &mut unsafe {
                    LowerContext::new(cx.as_context_mut(), read_options, types, instance)
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
    mut cx: StoreContextMut<T>,
    instance: *mut ComponentInstance,
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
            cx.0.id(),
            NonNull::new(memory),
            NonNull::new(realloc),
            StringEncoding::from_u8(string_encoding).unwrap(),
            true,
            None,
        )
    };
    let types = unsafe { (*instance).component_types() };
    let (rep, state) = unsafe { get_mut_by_index(&mut *instance, ty, handle)? };
    let StreamFutureState::Write = *state else {
        bail!("invalid handle");
    };
    *state = StreamFutureState::Busy;
    let transmit_id = TableId::<TransmitState>::new(rep);
    let transmit = cx.concurrent_state().table.get_mut(transmit_id)?;
    let new_state = if let ReadState::Closed(err_ctx) = &transmit.read {
        ReadState::Closed(*err_ctx)
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
            instance: _,
            handle: read_handle,
            caller: read_caller,
            ..
        } => {
            assert_eq!(flat_abi, read_flat_abi);

            let count = count.min(read_count);

            copy(
                cx.as_context_mut(),
                types,
                instance,
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

            log::trace!(
                "remove read child of {}: {}",
                read_caller.rep(),
                transmit_id.rep()
            );
            cx.concurrent_state()
                .table
                .remove_child(transmit_id, read_caller)?;

            unsafe {
                *get_mut_by_index(&mut *instance, read_ty, read_handle)?.1 =
                    StreamFutureState::Read;
            }

            push_event(
                cx,
                transmit_id.rep(),
                match read_ty {
                    TableIndex::Future(_) => Event::FutureRead,
                    TableIndex::Stream(_) => Event::StreamRead,
                },
                count,
                read_caller,
            );

            count
        }

        ReadState::HostReady { accept } => {
            let lift = &mut unsafe { LiftContext::new(cx.0, &options, types, instance) };
            accept(Writer::Guest {
                lift,
                ty: payload(ty, types),
                address,
                count,
            })?
        }

        ReadState::Open => {
            assert!(matches!(&transmit.write, WriteState::Open));

            let caller = cx.concurrent_state().guest_task.unwrap();
            log::trace!(
                "add write {} child of {}: {}",
                match ty {
                    TableIndex::Future(_) => "future",
                    TableIndex::Stream(_) => "stream",
                },
                caller.rep(),
                transmit_id.rep()
            );
            cx.concurrent_state().table.add_child(transmit_id, caller)?;

            let transmit = cx.concurrent_state().table.get_mut(transmit_id)?;
            transmit.write = WriteState::GuestReady {
                ty,
                flat_abi,
                options,
                address: usize::try_from(address).unwrap(),
                count: usize::try_from(count).unwrap(),
                instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
                handle,
                caller,
                post_write: PostWrite::Continue,
            };

            BLOCKED
        }

        // TODO: check
        // TODO: guest_write needs error context type
        ReadState::Closed(err_ctx) => {
            match err_ctx {
                // If no error context is provided, closed can be sent
                None => CLOSED,
                // If an error context was present, we must ensure it's created and bitwise OR w/ CLOSED
                Some(err_ctx) => {
                    // Lower the global error context that was saved into write state into a component-local
                    // error context handle
                    let state_tbl = unsafe {
                        (*instance)
                            .component_error_context_tables()
                            .get_mut(err_ctx_ty)
                            .context(
                                "retrieving local error context table during closed read w/ error",
                            )
                    }?;

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

                    // // NOTE: During write closure when the error context was provided, we
                    // // incremented the global count to ensure the error context would not be garbage collected,
                    // // if dropped by the sending component.
                    // //
                    // // Since we did that preemptively, we do not need to increment the global ref count even
                    // // after this increase in local ref count.
                    // //
                    // // If a reader (this reader) *never* comes along, when the relevant stream/future is closed,
                    // // the writer state will indicate that the global count must be amended.

                    // // We reset the write state to a simple closed now that the error value has been read out,
                    // // in the case that another read is performed
                    // cx.concurrent_state().table.get_mut(transmit_id)?.write =
                    //     WriteState::Closed(None);

                    CLOSED | local_err_ctx as usize
                }
            }
        }
    };

    if result != BLOCKED {
        unsafe {
            *get_mut_by_index(&mut *instance, ty, handle)?.1 = StreamFutureState::Write;
        }
    }

    Ok(u32::try_from(result).unwrap())
}

pub(super) fn guest_read<T>(
    mut cx: StoreContextMut<T>,
    instance: *mut ComponentInstance,
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
            cx.0.id(),
            NonNull::new(memory),
            NonNull::new(realloc),
            StringEncoding::from_u8(string_encoding).unwrap(),
            true,
            None,
        )
    };
    let types = unsafe { (*instance).component_types() };
    let (rep, state) = unsafe { get_mut_by_index(&mut *instance, ty, handle)? };
    let StreamFutureState::Read = *state else {
        bail!("invalid handle");
    };
    *state = StreamFutureState::Busy;
    let transmit_id = TableId::<TransmitState>::new(rep);
    let transmit = cx.concurrent_state().table.get_mut(transmit_id)?;
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
            instance: _,
            handle: write_handle,
            caller: write_caller,
            post_write,
        } => {
            assert_eq!(flat_abi, write_flat_abi);

            let count = count.min(write_count);

            copy(
                cx.as_context_mut(),
                types,
                instance,
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

            log::trace!(
                "remove write child of {}: {}",
                write_caller.rep(),
                transmit_id.rep()
            );
            cx.concurrent_state()
                .table
                .remove_child(transmit_id, write_caller)?;

            if let PostWrite::Close(err_ctx) = post_write {
                cx.concurrent_state().table.get_mut(transmit_id)?.write =
                    WriteState::Closed(err_ctx);
            } else {
                unsafe {
                    *get_mut_by_index(&mut *instance, write_ty, write_handle)?.1 =
                        StreamFutureState::Write;
                }
            }

            push_event(
                cx,
                transmit_id.rep(),
                match write_ty {
                    TableIndex::Future(_) => Event::FutureWrite,
                    TableIndex::Stream(_) => Event::StreamWrite,
                },
                count,
                write_caller,
            );

            count
        }

        WriteState::HostReady { accept, post_write } => {
            let count = accept(Reader::Guest {
                lower: RawLowerContext {
                    store: cx.0.traitobj().as_ptr(),
                    options: &options,
                    types,
                    instance,
                },
                ty,
                address: usize::try_from(address).unwrap(),
                count,
            })?;

            if let PostWrite::Close(err_ctx) = post_write {
                cx.concurrent_state().table.get_mut(transmit_id)?.write =
                    WriteState::Closed(err_ctx);
            }

            count
        }

        WriteState::Open => {
            assert!(matches!(&transmit.read, ReadState::Open));

            let caller = cx.concurrent_state().guest_task.unwrap();
            log::trace!(
                "add read {} child of {}: {}",
                match ty {
                    TableIndex::Future(_) => "future",
                    TableIndex::Stream(_) => "stream",
                },
                caller.rep(),
                transmit_id.rep()
            );
            cx.concurrent_state().table.add_child(transmit_id, caller)?;

            let transmit = cx.concurrent_state().table.get_mut(transmit_id)?;
            transmit.read = ReadState::GuestReady {
                ty,
                flat_abi,
                options,
                address: usize::try_from(address).unwrap(),
                count: usize::try_from(count).unwrap(),
                instance: SendSyncPtr::new(NonNull::new(instance).unwrap()),
                handle,
                caller,
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
                    let state_tbl = unsafe {
                        (*instance)
                            .component_error_context_tables()
                            .get_mut(err_ctx_ty)
                            .context(
                                "retrieving local error context table during closed read w/ error",
                            )
                    }?;

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
                    cx.concurrent_state().table.get_mut(transmit_id)?.write =
                        WriteState::Closed(None);

                    CLOSED | local_err_ctx as usize
                }
            }
        }
    };

    if result != BLOCKED {
        unsafe {
            *get_mut_by_index(&mut *instance, ty, handle)?.1 = StreamFutureState::Read;
        }
    }

    Ok(u32::try_from(result).unwrap())
}

pub(super) fn guest_cancel_write<T>(
    cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
    ty: TableIndex,
    writer: u32,
    _async_: bool,
) -> Result<u32> {
    let (rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
        state_table(instance, ty).get_mut_by_index(writer)?
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
    host_cancel_write(cx, rep)
}

pub(super) fn guest_cancel_read<T>(
    cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
    ty: TableIndex,
    reader: u32,
    _async_: bool,
) -> Result<u32> {
    let (rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
        state_table(instance, ty).get_mut_by_index(reader)?
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
    host_cancel_read(cx, rep)
}

pub(super) fn guest_close_writable<T>(
    mut cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
    ty: TableIndex,
    err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
    writer: u32,
    local_err_ctx: u32,
) -> Result<()> {
    let (transmit_rep, WaitableState::Stream(_, state) | WaitableState::Future(_, state)) =
        state_table(instance, ty)
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
            let state_tbl = (*instance)
                .component_error_context_tables()
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
            let GlobalErrorContextRefCount(global_count) = (*instance)
                .component_global_error_context_ref_counts()
                .get_mut(&global_err_ctx)
                .context("retrieving global error context ref count during guest close writable")?;
            *global_count += 1;
            ensure!(
                cx.concurrent_state()
                    .table
                    .get(TableId::<ErrorContextState>::new(rep))
                    .is_ok(),
                "missing global error context state [{rep}] for local error context [{err_ctx}] during guest close writable"
            );
            Some(global_err_ctx)
        }
    };

    host_close_writer(cx, transmit_rep, global_err_ctx)
}

pub(super) fn guest_close_readable<T>(
    mut cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
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
        state_table(instance, ty).remove_by_index(reader)?
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

    let global_err_ctx = lookup_local_error_context(&mut cx, instance, err_ctx_ty, error)
        .context("local error context lookup")?
        .map(|v| v.as_u32());
    host_close_reader(cx, rep, global_err_ctx)
}

/// Create a new error context for the given component
pub(super) fn error_context_new<T>(
    mut cx: StoreContextMut<T>,
    instance: *mut ComponentInstance,
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
            cx.0.id(),
            NonNull::new(memory),
            NonNull::new(realloc),
            StringEncoding::from_u8(string_encoding).ok_or_else(|| {
                anyhow::anyhow!("failed to convert u8 string encoding [{string_encoding}]")
            })?,
            false,
            None,
        )
    };
    let lift_ctx =
        &mut unsafe { LiftContext::new(cx.0, &options, (*instance).component_types(), instance) };
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
                .map(|_| [debug_msg_address.to_le_bytes(), debug_msg_len.to_le_bytes()].concat())
                .ok_or_else(|| anyhow::anyhow!("invalid debug message pointer: out of bounds"))?,
        )?
    };

    // Create a new ErrorContext that is tracked along with other concurrent state
    let err_ctx = ErrorContextState {
        debug_msg: s.to_str(&cx)?.to_string(),
    };
    let table_id = cx.concurrent_state().table.push(err_ctx)?;
    let global_ref_count_idx = TypeComponentGlobalErrorContextTableIndex::from_u32(table_id.rep());

    // Add to the global error context ref counts
    unsafe {
        let _ = (*instance)
            .component_global_error_context_ref_counts()
            .insert(global_ref_count_idx, GlobalErrorContextRefCount(1));
    }

    // Error context are tracked both locally (to a single component instance) and globally
    // the counts for both must stay in sync.
    //
    // Here we reflect the newly created global concurrent error context state into the
    // component instance's locally tracked count, along with the appropriate key into the global
    // ref tracking data structures to enable later lookup
    let local_tbl = unsafe {
        (*instance)
            .component_error_context_tables()
            .get_mut_or_insert_with(ty, || StateTable::default())
    };
    assert!(
        !local_tbl.has_handle(table_id.rep()),
        "newly created error context state already tracked by component"
    );
    let local_idx = local_tbl.insert(table_id.rep(), LocalErrorContextRefCount(1))?;

    Ok(local_idx)
}

pub(super) fn error_context_debug_message<T>(
    mut cx: StoreContextMut<T>,
    instance: *mut ComponentInstance,
    memory: *mut VMMemoryDefinition,
    realloc: *mut VMFuncRef,
    string_encoding: u8,
    ty: TypeComponentLocalErrorContextTableIndex,
    err_ctx_handle: u32,
    debug_msg_address: u32,
) -> Result<()> {
    let store_id = cx.0.id();

    // Retrieve the error context and internal debug message
    let (state_table_id_rep, _) = unsafe {
        (*instance)
            .component_error_context_tables()
            .get_mut(ty)
            .context("error context table index present in (sub)component lookup during debug_msg")?
            .get_mut_by_index(err_ctx_handle)?
    };

    // Get the state associated with the error context
    let ErrorContextState { debug_msg } =
        cx.concurrent_state()
            .table
            .get_mut(TableId::<ErrorContextState>::new(state_table_id_rep))?;
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
    let lower_cx =
        &mut unsafe { LowerContext::new(cx, &options, (*instance).component_types(), instance) };
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

pub(super) fn error_context_drop<T>(
    mut cx: StoreContextMut<T>,
    instance: &mut ComponentInstance,
    ty: TypeComponentLocalErrorContextTableIndex,
    error_context: u32,
) -> Result<()> {
    let local_state_table = instance
        .component_error_context_tables()
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

    let GlobalErrorContextRefCount(global_ref_count) = instance
        .component_global_error_context_ref_counts()
        .get_mut(&global_ref_count_idx)
        .expect("retrieve concurrent state for error context during drop");

    // Reduce the component-global ref count, removing tracking if necessary
    assert!(*global_ref_count >= 1);
    *global_ref_count -= 1;
    if *global_ref_count == 0 {
        assert!(local_ref_removed);

        instance
            .component_global_error_context_ref_counts()
            .remove(&global_ref_count_idx);

        cx.concurrent_state()
            .table
            .delete(TableId::<ErrorContextState>::new(rep))
            .context("deleting component-global error context data")?;
    }

    Ok(())
}

/// Resolve a provided component-local error context index to a component-global index
pub(super) fn lookup_local_error_context<T>(
    cx: &mut StoreContextMut<T>,
    instance: &mut ComponentInstance,
    err_ctx_ty: TypeComponentLocalErrorContextTableIndex,
    local_err_ctx: u32,
) -> Result<Option<TypeComponentGlobalErrorContextTableIndex>> {
    // Resolve the error context
    match local_err_ctx {
        // If no error context was provided, we can pass that along as-is
        0 => Ok(None),

        // If a non-zero error context was provided, first ensure it's valid,
        // then lift the guest-local (component instance local) error context reference
        // to the component-global level.
        //
        // This ensures that after closing the writer, when the eventual reader appears
        // we can lower the component-global error context into a reader-local error context
        err_ctx => {
            // Look up the local component error context
            let state_tbl = (*instance)
                .component_error_context_tables()
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
            let GlobalErrorContextRefCount(global_count) = (*instance)
                .component_global_error_context_ref_counts()
                .get_mut(&global_err_ctx)
                .context("retrieving global error context ref count during guest close writable")?;
            *global_count += 1;
            ensure!(
                cx.concurrent_state()
                    .table
                    .get(TableId::<ErrorContextState>::new(rep))
                    .is_ok(),
                "missing global error context state [{rep}] for local error context [{err_ctx}] during guest close writable"
            );
            Ok(Some(global_err_ctx))
        }
    }
}
