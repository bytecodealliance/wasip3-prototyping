#[cfg(feature = "call-hook")]
use crate::CallHook;
use crate::prelude::*;
use crate::runtime::vm::mpk::{self, ProtectionMask};
use crate::store::{ResourceLimiterInner, StoreInner, StoreOpaque};
use crate::{Engine, Store, StoreContextMut, UpdateDeadline};
use anyhow::Context as _;
use core::cell::UnsafeCell;
use core::future::Future;
use core::ops::Range;
use core::pin::{Pin, pin};
use core::ptr;
use core::task::{Context, Poll};

/// An object that can take callbacks when the runtime enters or exits hostcalls.
#[cfg(feature = "call-hook")]
#[async_trait::async_trait]
pub trait CallHookHandler<T>: Send {
    /// A callback to run when wasmtime is about to enter a host call, or when about to
    /// exit the hostcall.
    async fn handle_call_event(&self, t: StoreContextMut<'_, T>, ch: CallHook) -> Result<()>;
}

pub struct AsyncState {
    current_suspend: UnsafeCell<*mut wasmtime_fiber::Suspend<Result<()>, (), Result<()>>>,
    current_poll_cx: UnsafeCell<PollContext>,
    /// The last fiber stack that was in use by this store.
    last_fiber_stack: Option<wasmtime_fiber::FiberStack>,
}

impl Default for AsyncState {
    fn default() -> AsyncState {
        AsyncState {
            current_suspend: UnsafeCell::new(ptr::null_mut()),
            current_poll_cx: UnsafeCell::new(PollContext::default()),
            last_fiber_stack: None,
        }
    }
}

// Lots of pesky unsafe cells and pointers in this structure. This means we need
// to declare explicitly that we use this in a threadsafe fashion.
unsafe impl Send for AsyncState {}
unsafe impl Sync for AsyncState {}

#[derive(Clone, Copy)]
struct PollContext {
    future_context: *mut Context<'static>,
    #[cfg_attr(feature = "component-model-async", allow(dead_code))]
    guard_range_start: *mut u8,
    #[cfg_attr(feature = "component-model-async", allow(dead_code))]
    guard_range_end: *mut u8,
}

impl Default for PollContext {
    fn default() -> PollContext {
        PollContext {
            future_context: core::ptr::null_mut(),
            guard_range_start: core::ptr::null_mut(),
            guard_range_end: core::ptr::null_mut(),
        }
    }
}

impl<T> Store<T> {
    /// Configures the [`ResourceLimiterAsync`](crate::ResourceLimiterAsync)
    /// used to limit resource creation within this [`Store`].
    ///
    /// This method is an asynchronous variant of the [`Store::limiter`] method
    /// where the embedder can block the wasm request for more resources with
    /// host `async` execution of futures.
    ///
    /// By using a [`ResourceLimiterAsync`](`crate::ResourceLimiterAsync`)
    /// with a [`Store`], you can no longer use
    /// [`Memory::new`](`crate::Memory::new`),
    /// [`Memory::grow`](`crate::Memory::grow`),
    /// [`Table::new`](`crate::Table::new`), and
    /// [`Table::grow`](`crate::Table::grow`). Instead, you must use their
    /// `async` variants: [`Memory::new_async`](`crate::Memory::new_async`),
    /// [`Memory::grow_async`](`crate::Memory::grow_async`),
    /// [`Table::new_async`](`crate::Table::new_async`), and
    /// [`Table::grow_async`](`crate::Table::grow_async`).
    ///
    /// Note that this limiter is only used to limit the creation/growth of
    /// resources in the future, this does not retroactively attempt to apply
    /// limits to the [`Store`]. Additionally this must be used with an async
    /// [`Store`] configured via
    /// [`Config::async_support`](crate::Config::async_support).
    pub fn limiter_async(
        &mut self,
        mut limiter: impl FnMut(&mut T) -> &mut (dyn crate::ResourceLimiterAsync)
        + Send
        + Sync
        + 'static,
    ) {
        debug_assert!(self.inner.async_support());
        // Apply the limits on instances, tables, and memory given by the limiter:
        let inner = &mut self.inner;
        let (instance_limit, table_limit, memory_limit) = {
            let l = limiter(&mut inner.data);
            (l.instances(), l.tables(), l.memories())
        };
        let innermost = &mut inner.inner;
        innermost.instance_limit = instance_limit;
        innermost.table_limit = table_limit;
        innermost.memory_limit = memory_limit;

        // Save the limiter accessor function:
        inner.limiter = Some(ResourceLimiterInner::Async(Box::new(limiter)));
    }

    /// Configures an async function that runs on calls and returns between
    /// WebAssembly and host code. For the non-async equivalent of this method,
    /// see [`Store::call_hook`].
    ///
    /// The function is passed a [`CallHook`] argument, which indicates which
    /// state transition the VM is making.
    ///
    /// This function's future may return a [`Trap`]. If a trap is returned
    /// when an import was called, it is immediately raised as-if the host
    /// import had returned the trap. If a trap is returned after wasm returns
    /// to the host then the wasm function's result is ignored and this trap is
    /// returned instead.
    ///
    /// After this function returns a trap, it may be called for subsequent
    /// returns to host or wasm code as the trap propagates to the root call.
    #[cfg(feature = "call-hook")]
    pub fn call_hook_async(&mut self, hook: impl CallHookHandler<T> + Send + Sync + 'static) {
        self.inner.call_hook = Some(crate::store::CallHookInner::Async(Box::new(hook)));
    }

    /// Perform garbage collection asynchronously.
    ///
    /// Note that it is not required to actively call this function. GC will
    /// automatically happen according to various internal heuristics. This is
    /// provided if fine-grained control over the GC is desired.
    ///
    /// This method is only available when the `gc` Cargo feature is enabled.
    #[cfg(feature = "gc")]
    pub async fn gc_async(&mut self, why: Option<&crate::GcHeapOutOfMemory<()>>) -> Result<()>
    where
        T: Send,
    {
        self.inner.gc_async(why).await
    }

    /// Configures epoch-deadline expiration to yield to the async
    /// caller and the update the deadline.
    ///
    /// When epoch-interruption-instrumented code is executed on this
    /// store and the epoch deadline is reached before completion,
    /// with the store configured in this way, execution will yield
    /// (the future will return `Pending` but re-awake itself for
    /// later execution) and, upon resuming, the store will be
    /// configured with an epoch deadline equal to the current epoch
    /// plus `delta` ticks.
    ///
    /// This setting is intended to allow for cooperative timeslicing
    /// of multiple CPU-bound Wasm guests in different stores, all
    /// executing under the control of an async executor. To drive
    /// this, stores should be configured to "yield and update"
    /// automatically with this function, and some external driver (a
    /// thread that wakes up periodically, or a timer
    /// signal/interrupt) should call
    /// [`Engine::increment_epoch()`](crate::Engine::increment_epoch).
    ///
    /// See documentation on
    /// [`Config::epoch_interruption()`](crate::Config::epoch_interruption)
    /// for an introduction to epoch-based interruption.
    #[cfg(target_has_atomic = "64")]
    pub fn epoch_deadline_async_yield_and_update(&mut self, delta: u64) {
        self.inner.epoch_deadline_async_yield_and_update(delta);
    }
}

impl<'a, T> StoreContextMut<'a, T> {
    /// Perform garbage collection of `ExternRef`s.
    ///
    /// Same as [`Store::gc`].
    ///
    /// This method is only available when the `gc` Cargo feature is enabled.
    #[cfg(feature = "gc")]
    pub async fn gc_async(&mut self, why: Option<&crate::GcHeapOutOfMemory<()>>) -> Result<()>
    where
        T: Send + 'static,
    {
        self.0.gc_async(why).await
    }

    /// Configures epoch-deadline expiration to yield to the async
    /// caller and the update the deadline.
    ///
    /// For more information see
    /// [`Store::epoch_deadline_async_yield_and_update`].
    #[cfg(target_has_atomic = "64")]
    pub fn epoch_deadline_async_yield_and_update(&mut self, delta: u64) {
        self.0.epoch_deadline_async_yield_and_update(delta);
    }
}

impl<T> StoreInner<T> {
    /// Yields execution to the caller on out-of-gas or epoch interruption.
    ///
    /// This only works on async futures and stores, and assumes that we're
    /// executing on a fiber. This will yield execution back to the caller once.
    pub fn async_yield_impl(&mut self) -> Result<()> {
        // When control returns, we have a `Result<()>` passed
        // in from the host fiber. If this finished successfully then
        // we were resumed normally via a `poll`, so keep going.  If
        // the future was dropped while we were yielded, then we need
        // to clean up this fiber. Do so by raising a trap which will
        // abort all wasm and get caught on the other side to clean
        // things up.
        self.block_on(|_| crate::runtime::vm::Yield::new())
    }

    #[cfg(target_has_atomic = "64")]
    fn epoch_deadline_async_yield_and_update(&mut self, delta: u64) {
        assert!(
            self.async_support(),
            "cannot use `epoch_deadline_async_yield_and_update` without enabling async support in the config"
        );
        self.epoch_deadline_behavior =
            Some(Box::new(move |_store| Ok(UpdateDeadline::Yield(delta))));
    }

    pub(crate) fn block_on<'a, F: Future + Send>(
        &'a mut self,
        mk_future: impl FnOnce(&'a mut Self) -> F,
    ) -> Result<F::Output> {
        #[cfg(feature = "component-model-async")]
        {
            let async_cx = crate::component::concurrent::AsyncCx::try_new(self)
                .context("couldn't create AsyncCx to block on async operation")?;
            let future = mk_future(self);
            let mut future = core::pin::pin!(future);
            unsafe { Ok(async_cx.block_on(future.as_mut(), None)?.0) }
        }
        #[cfg(not(feature = "component-model-async"))]
        {
            let cx = self
                .inner
                .async_cx()
                .context("async_cx is not present to block on async operation")?;
            let future = mk_future(self);
            let mut future = core::pin::pin!(future);
            unsafe { cx.block_on(future.as_mut()) }
        }
    }
}

#[doc(hidden)]
impl StoreOpaque {
    /// Executes a synchronous computation `func` asynchronously on a new fiber.
    ///
    /// This function will convert the synchronous `func` into an asynchronous
    /// future. This is done by running `func` in a fiber on a separate native
    /// stack which can be suspended and resumed from.
    ///
    /// Most of the nitty-gritty here is how we juggle the various contexts
    /// necessary to suspend the fiber later on and poll sub-futures. It's hoped
    /// that the various comments are illuminating as to what's going on here.
    pub(crate) async fn on_fiber<R>(
        &mut self,
        func: impl FnOnce(&mut Self) -> R + Send,
    ) -> Result<R> {
        let config = self.engine().config();
        debug_assert!(self.async_support());
        debug_assert!(config.async_stack_size > 0);

        let mut slot = None;
        let mut future = {
            let current_poll_cx = self.async_state.current_poll_cx.get();
            let current_suspend = self.async_state.current_suspend.get();
            let stack = self.allocate_fiber_stack()?;
            let track_pkey_context_switch = self.pkey.is_some();

            let engine = self.engine().clone();
            let slot = &mut slot;
            let this = &mut *self;
            let fiber = wasmtime_fiber::Fiber::new(stack, move |keep_going, suspend| {
                // First check and see if we were interrupted/dropped, and only
                // continue if we haven't been.
                keep_going?;

                // Configure our store's suspension context for the rest of the
                // execution of this fiber. Note that a raw pointer is stored here
                // which is only valid for the duration of this closure.
                // Consequently we at least replace it with the previous value when
                // we're done. This reset is also required for correctness because
                // otherwise our value will overwrite another active fiber's value.
                // There should be a test that segfaults in `async_functions.rs` if
                // this `Replace` is removed.
                unsafe {
                    let _reset = Reset(current_suspend, *current_suspend);
                    *current_suspend = suspend;

                    *slot = Some(func(this));
                    Ok(())
                }
            })?;

            // Once we have the fiber representing our synchronous computation, we
            // wrap that in a custom future implementation which does the
            // translation from the future protocol to our fiber API.
            FiberFuture {
                fiber: Some(fiber),
                current_poll_cx,
                engine,
                fiber_resume_state: Some(FiberResumeState {
                    tls: crate::runtime::vm::AsyncWasmCallState::new(),
                    mpk: if track_pkey_context_switch {
                        Some(ProtectionMask::all())
                    } else {
                        None
                    },
                }),
            }
        };
        (&mut future).await?;
        let stack = future.fiber.take().map(|f| f.into_stack());
        drop(future);
        if let Some(stack) = stack {
            self.deallocate_fiber_stack(stack);
        }

        return Ok(slot.unwrap());

        struct FiberFuture<'a> {
            fiber: Option<wasmtime_fiber::Fiber<'a, Result<()>, (), Result<()>>>,
            current_poll_cx: *mut PollContext,
            engine: Engine,
            // See comments in `FiberResumeState` for this
            fiber_resume_state: Option<FiberResumeState>,
        }

        // This is surely the most dangerous `unsafe impl Send` in the entire
        // crate. There are two members in `FiberFuture` which cause it to not
        // be `Send`. One is `current_poll_cx` and is entirely uninteresting.
        // This is just used to manage `Context` pointers across `await` points
        // in the future, and requires raw pointers to get it to happen easily.
        // Nothing too weird about the `Send`-ness, values aren't actually
        // crossing threads.
        //
        // The really interesting piece is `fiber`. Now the "fiber" here is
        // actual honest-to-god Rust code which we're moving around. What we're
        // doing is the equivalent of moving our thread's stack to another OS
        // thread. Turns out we, in general, have no idea what's on the stack
        // and would generally have no way to verify that this is actually safe
        // to do!
        //
        // Thankfully, though, Wasmtime has the power. Without being glib it's
        // actually worth examining what's on the stack. It's unfortunately not
        // super-local to this function itself. Our closure to `Fiber::new` runs
        // `func`, which is given to us from the outside. Thankfully, though, we
        // have tight control over this. Usage of `on_fiber` is typically done
        // *just* before entering WebAssembly itself, so we'll have a few stack
        // frames of Rust code (all in Wasmtime itself) before we enter wasm.
        //
        // Once we've entered wasm, well then we have a whole bunch of wasm
        // frames on the stack. We've got this nifty thing called Cranelift,
        // though, which allows us to also have complete control over everything
        // on the stack!
        //
        // Finally, when wasm switches back to the fiber's starting pointer
        // (this future we're returning) then it means wasm has reentered Rust.
        // Suspension can only happen via the `block_on` function of an
        // `AsyncCx`. This, conveniently, also happens entirely in Wasmtime
        // controlled code!
        //
        // There's an extremely important point that should be called out here.
        // User-provided futures **are not on the stack** during suspension
        // points. This is extremely crucial because we in general cannot reason
        // about Send/Sync for stack-local variables since rustc doesn't analyze
        // them at all. With our construction, though, we are guaranteed that
        // Wasmtime owns all stack frames between the stack of a fiber and when
        // the fiber suspends (and it could move across threads). At this time
        // the only user-provided piece of data on the stack is the future
        // itself given to us. Lo-and-behold as you might notice the future is
        // required to be `Send`!
        //
        // What this all boils down to is that we, as the authors of Wasmtime,
        // need to be extremely careful that on the async fiber stack we only
        // store Send things. For example we can't start using `Rc` willy nilly
        // by accident and leave a copy in TLS somewhere. (similarly we have to
        // be ready for TLS to change while we're executing wasm code between
        // suspension points).
        //
        // While somewhat onerous it shouldn't be too too hard (the TLS bit is
        // the hardest bit so far). This does mean, though, that no user should
        // ever have to worry about the `Send`-ness of Wasmtime. If rustc says
        // it's ok, then it's ok.
        //
        // With all that in mind we unsafely assert here that wasmtime is
        // correct. We declare the fiber as only containing Send data on its
        // stack, despite not knowing for sure at compile time that this is
        // correct. That's what `unsafe` in Rust is all about, though, right?
        unsafe impl Send for FiberFuture<'_> {}

        impl FiberFuture<'_> {
            fn fiber(&self) -> &wasmtime_fiber::Fiber<'_, Result<()>, (), Result<()>> {
                self.fiber.as_ref().unwrap()
            }

            /// This is a helper function to call `resume` on the underlying
            /// fiber while correctly managing Wasmtime's state that the fiber
            /// may clobber.
            ///
            /// ## Return Value
            ///
            /// * `Ok(Ok(()))` - the fiber successfully completed and yielded a
            ///    successful result.
            /// * `Ok(Err(e))` - the fiber successfully completed and yielded
            ///   an error as a result of computation.
            /// * `Err(())` - the fiber has not finished and it is suspended.
            fn resume(&mut self, val: Result<()>) -> Result<Result<()>, ()> {
                unsafe {
                    let prev = self.fiber_resume_state.take().unwrap().replace();
                    let restore = Restore {
                        fiber: self,
                        prior_fiber_state: Some(prev),
                    };
                    return restore.fiber.fiber().resume(val);
                }

                struct Restore<'a, 'b> {
                    fiber: &'a mut FiberFuture<'b>,
                    prior_fiber_state: Option<PriorFiberResumeState>,
                }

                impl Drop for Restore<'_, '_> {
                    fn drop(&mut self) {
                        unsafe {
                            self.fiber.fiber_resume_state =
                                Some(self.prior_fiber_state.take().unwrap().replace());
                        }
                    }
                }
            }
        }

        impl Future for FiberFuture<'_> {
            type Output = Result<()>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
                // We need to carry over this `cx` into our fiber's runtime
                // for when it tries to poll sub-futures that are created. Doing
                // this must be done unsafely, however, since `cx` is only alive
                // for this one singular function call. Here we do a `transmute`
                // to extend the lifetime of `Context` so it can be stored in
                // our `Store`, and then we replace the current polling context
                // with this one.
                //
                // Note that the replace is done for weird situations where
                // futures might be switching contexts and there's multiple
                // wasmtime futures in a chain of futures.
                //
                // On exit from this function, though, we reset the polling
                // context back to what it was to signify that `Store` no longer
                // has access to this pointer.
                let guard = self
                    .fiber()
                    .stack()
                    .guard_range()
                    .unwrap_or(core::ptr::null_mut()..core::ptr::null_mut());
                unsafe {
                    let _reset = Reset(self.current_poll_cx, *self.current_poll_cx);
                    *self.current_poll_cx = PollContext {
                        future_context: core::mem::transmute::<
                            &mut Context<'_>,
                            *mut Context<'static>,
                        >(cx),
                        guard_range_start: guard.start,
                        guard_range_end: guard.end,
                    };

                    // After that's set up we resume execution of the fiber, which
                    // may also start the fiber for the first time. This either
                    // returns `Ok` saying the fiber finished (yay!) or it
                    // returns `Err` with the payload passed to `suspend`, which
                    // in our case is `()`.
                    match self.resume(Ok(())) {
                        Ok(result) => Poll::Ready(result),

                        // If `Err` is returned that means the fiber polled a
                        // future but it said "Pending", so we propagate that
                        // here.
                        //
                        // An additional safety check is performed when leaving
                        // this function to help bolster the guarantees of
                        // `unsafe impl Send` above. Notably this future may get
                        // re-polled on a different thread. Wasmtime's
                        // thread-local state points to the stack, however,
                        // meaning that it would be incorrect to leave a pointer
                        // in TLS when this function returns. This function
                        // performs a runtime assert to verify that this is the
                        // case, notably that the one TLS pointer Wasmtime uses
                        // is not pointing anywhere within the stack. If it is
                        // then that's a bug indicating that TLS management in
                        // Wasmtime is incorrect.
                        Err(()) => {
                            if let Some(range) = self.fiber().stack().range() {
                                crate::runtime::vm::AsyncWasmCallState::assert_current_state_not_in_range(range);
                            }
                            Poll::Pending
                        }
                    }
                }
            }
        }

        // Dropping futures is pretty special in that it means the future has
        // been requested to be cancelled. Here we run the risk of dropping an
        // in-progress fiber, and if we were to do nothing then the fiber would
        // leak all its owned stack resources.
        //
        // To handle this we implement `Drop` here and, if the fiber isn't done,
        // resume execution of the fiber saying "hey please stop you're
        // interrupted". Our `Trap` created here (which has the stack trace
        // of whomever dropped us) will then get propagated in whatever called
        // `block_on`, and the idea is that the trap propagates all the way back
        // up to the original fiber start, finishing execution.
        //
        // We don't actually care about the fiber's return value here (no one's
        // around to look at it), we just assert the fiber finished to
        // completion.
        impl Drop for FiberFuture<'_> {
            fn drop(&mut self) {
                if self.fiber.is_none() {
                    return;
                }

                if !self.fiber().done() {
                    let result = self.resume(Err(anyhow!("future dropped")));
                    // This resumption with an error should always complete the
                    // fiber. While it's technically possible for host code to
                    // catch the trap and re-resume, we'd ideally like to
                    // signal that to callers that they shouldn't be doing
                    // that.
                    debug_assert!(result.is_ok());

                    // Note that `result` is `Ok(r)` where `r` is either
                    // `Ok(())` or `Err(e)`. If it's an error that's disposed of
                    // here. It's expected to be a propagation of the `future
                    // dropped` error created above.
                }

                self.fiber_resume_state.take().unwrap().dispose();

                unsafe {
                    self.engine
                        .allocator()
                        .deallocate_fiber_stack(self.fiber.take().unwrap().into_stack());
                }
            }
        }

        /// State of the world when a fiber last suspended.
        ///
        /// This structure represents global state that a fiber clobbers during
        /// its execution. For example TLS variables are updated, system
        /// resources like MPK masks are updated, etc. The purpose of this
        /// structure is to track all of this state and appropriately
        /// save/restore it around fiber suspension points.
        struct FiberResumeState {
            /// Saved list of `CallThreadState` activations that are stored on a
            /// fiber stack.
            ///
            /// This is a linked list that references stack-stored nodes on the
            /// fiber stack that is currently suspended. The
            /// `AsyncWasmCallState` type documents this more thoroughly but the
            /// general gist is that when we this fiber is resumed this linked
            /// list needs to be pushed on to the current thread's linked list
            /// of activations.
            tls: crate::runtime::vm::AsyncWasmCallState,

            /// Saved MPK protection mask, if enabled.
            ///
            /// When MPK is enabled then executing WebAssembly will modify the
            /// processor's current mask of addressable protection keys. This
            /// means that our current state may get clobbered when a fiber
            /// suspends. To ensure that this function preserves context it
            /// will, when MPK is enabled, save the current mask when this
            /// function is called and then restore the mask when the function
            /// returns (aka the fiber suspends).
            mpk: Option<ProtectionMask>,
        }

        impl FiberResumeState {
            unsafe fn replace(self) -> PriorFiberResumeState {
                let tls = self.tls.push();
                let mpk = swap_mpk_states(self.mpk);
                PriorFiberResumeState { tls, mpk }
            }

            fn dispose(self) {
                self.tls.assert_null();
            }
        }

        struct PriorFiberResumeState {
            tls: crate::runtime::vm::PreviousAsyncWasmCallState,
            mpk: Option<ProtectionMask>,
        }

        impl PriorFiberResumeState {
            unsafe fn replace(self) -> FiberResumeState {
                let tls = self.tls.restore();
                let mpk = swap_mpk_states(self.mpk);
                FiberResumeState { tls, mpk }
            }
        }

        fn swap_mpk_states(mask: Option<ProtectionMask>) -> Option<ProtectionMask> {
            mask.map(|mask| {
                let current = mpk::current_mask();
                mpk::allow(mask);
                current
            })
        }
    }

    #[cfg(feature = "gc")]
    pub(super) async fn do_gc_async(&mut self) {
        assert!(
            self.async_support(),
            "cannot use `gc_async` without enabling async support in the config",
        );

        // If the GC heap hasn't been initialized, there is nothing to collect.
        if self.gc_store.is_none() {
            return;
        }

        log::trace!("============ Begin Async GC ===========");

        // Take the GC roots out of `self` so we can borrow it mutably but still
        // call mutable methods on `self`.
        let mut roots = core::mem::take(&mut self.gc_roots_list);

        self.trace_roots_async(&mut roots).await;
        self.unwrap_gc_store_mut()
            .gc_async(unsafe { roots.iter() })
            .await;

        // Restore the GC roots for the next GC.
        roots.clear();
        self.gc_roots_list = roots;

        log::trace!("============ End Async GC ===========");
    }

    #[inline]
    #[cfg(not(feature = "gc"))]
    pub async fn gc_async(&mut self) {
        // Nothing to collect.
        //
        // Note that this is *not* a public method, this is just defined for the
        // crate-internal `StoreOpaque` type. This is a convenience so that we
        // don't have to `cfg` every call site.
    }

    #[cfg(feature = "gc")]
    async fn trace_roots_async(&mut self, gc_roots_list: &mut crate::runtime::vm::GcRootsList) {
        use crate::runtime::vm::Yield;

        log::trace!("Begin trace GC roots");

        // We shouldn't have any leftover, stale GC roots.
        assert!(gc_roots_list.is_empty());

        self.trace_wasm_stack_roots(gc_roots_list);
        Yield::new().await;
        self.trace_vmctx_roots(gc_roots_list);
        Yield::new().await;
        self.trace_user_roots(gc_roots_list);

        log::trace!("End trace GC roots")
    }

    /// Yields the async context, assuming that we are executing on a fiber and
    /// that fiber is not in the process of dying. This function will return
    /// None in the latter case (the fiber is dying), and panic if
    /// `async_support()` is false.
    #[inline]
    pub fn async_cx(&self) -> Option<AsyncCx> {
        assert!(self.async_support());

        let poll_cx_box_ptr = self.async_state.current_poll_cx.get();
        if poll_cx_box_ptr.is_null() {
            return None;
        }

        let poll_cx_inner_ptr = unsafe { *poll_cx_box_ptr };
        if poll_cx_inner_ptr.future_context.is_null() {
            return None;
        }

        Some(AsyncCx {
            current_suspend: self.async_state.current_suspend.get(),
            current_poll_cx: unsafe { &raw mut (*poll_cx_box_ptr).future_context },
        })
    }

    /// Yields execution to the caller on out-of-gas or epoch interruption.
    ///
    /// This only works on async futures and stores, and assumes that we're
    /// executing on a fiber. This will yield execution back to the caller once.
    pub fn async_yield_impl(&mut self) -> Result<()> {
        use crate::runtime::vm::Yield;

        let mut future = Yield::new();

        // When control returns, we have a `Result<()>` passed
        // in from the host fiber. If this finished successfully then
        // we were resumed normally via a `poll`, so keep going.  If
        // the future was dropped while we were yielded, then we need
        // to clean up this fiber. Do so by raising a trap which will
        // abort all wasm and get caught on the other side to clean
        // things up.
        unsafe {
            self.async_cx()
                .expect("attempted to pull async context during shutdown")
                .block_on(Pin::new_unchecked(&mut future))
        }
    }

    fn allocate_fiber_stack(&mut self) -> Result<wasmtime_fiber::FiberStack> {
        if let Some(stack) = self.async_state.last_fiber_stack.take() {
            return Ok(stack);
        }
        self.engine().allocator().allocate_fiber_stack()
    }

    fn deallocate_fiber_stack(&mut self, stack: wasmtime_fiber::FiberStack) {
        self.flush_fiber_stack();
        self.async_state.last_fiber_stack = Some(stack);
    }

    /// Releases the last fiber stack to the underlying instance allocator, if
    /// present.
    pub fn flush_fiber_stack(&mut self) {
        if let Some(stack) = self.async_state.last_fiber_stack.take() {
            unsafe {
                self.engine.allocator().deallocate_fiber_stack(stack);
            }
        }
    }

    pub(crate) fn async_guard_range(&mut self) -> Range<*mut u8> {
        #[cfg(feature = "component-model-async")]
        {
            unsafe { (*self.concurrent_async_state()).async_guard_range() }
        }
        #[cfg(not(feature = "component-model-async"))]
        unsafe {
            let ptr = self.async_state.current_poll_cx.get();
            (*ptr).guard_range_start..(*ptr).guard_range_end
        }
    }
}

impl<T> StoreContextMut<'_, T> {
    /// Executes a synchronous computation `func` asynchronously on a new fiber.
    pub(crate) async fn on_fiber<R>(
        &mut self,
        func: impl FnOnce(&mut StoreContextMut<'_, T>) -> R + Send,
    ) -> Result<R>
    where
        T: Send + 'static,
    {
        self.0
            .on_fiber(|opaque| {
                let store = unsafe { opaque.traitobj().cast::<StoreInner<T>>().as_mut() };
                func(&mut StoreContextMut(store))
            })
            .await
    }
}

pub struct AsyncCx {
    current_suspend: *mut *mut wasmtime_fiber::Suspend<Result<()>, (), Result<()>>,
    current_poll_cx: *mut *mut Context<'static>,
}

impl AsyncCx {
    /// Blocks on the asynchronous computation represented by `future` and
    /// produces the result here, in-line.
    ///
    /// This function is designed to only work when it's currently executing on
    /// a native fiber. This fiber provides the ability for us to handle the
    /// future's `Pending` state as "jump back to whomever called the fiber in
    /// an asynchronous fashion and propagate `Pending`". This tight coupling
    /// with `on_fiber` below is what powers the asynchronicity of calling wasm.
    /// Note that the asynchronous part only applies to host functions, wasm
    /// itself never really does anything asynchronous at this time.
    ///
    /// This function takes a `future` and will (appear to) synchronously wait
    /// on the result. While this function is executing it will fiber switch
    /// to-and-from the original frame calling `on_fiber` which should be a
    /// guarantee due to how async stores are configured.
    ///
    /// The return value here is either the output of the future `T`, or a trap
    /// which represents that the asynchronous computation was cancelled. It is
    /// not recommended to catch the trap and try to keep executing wasm, so
    /// we've tried to liberally document this.
    pub unsafe fn block_on<F>(&self, future: F) -> Result<F::Output>
    where
        F: Future + Send,
    {
        let mut future = pin!(future);

        // Take our current `Suspend` context which was configured as soon as
        // our fiber started. Note that we must load it at the front here and
        // save it on our stack frame. While we're polling the future other
        // fibers may be started for recursive computations, and the current
        // suspend context is only preserved at the edges of the fiber, not
        // during the fiber itself.
        //
        // For a little bit of extra safety we also replace the current value
        // with null to try to catch any accidental bugs on our part early.
        // This is all pretty unsafe so we're trying to be careful...
        //
        // Note that there should be a segfaulting test  in `async_functions.rs`
        // if this `Reset` is removed.
        let suspend = *self.current_suspend;
        let _reset = Reset(self.current_suspend, suspend);
        *self.current_suspend = ptr::null_mut();
        assert!(!suspend.is_null());

        loop {
            let future_result = {
                let poll_cx = *self.current_poll_cx;
                let _reset = Reset(self.current_poll_cx, poll_cx);
                *self.current_poll_cx = ptr::null_mut();
                assert!(!poll_cx.is_null());
                future.as_mut().poll(&mut *poll_cx)
            };

            match future_result {
                Poll::Ready(t) => break Ok(t),
                Poll::Pending => {}
            }

            (*suspend).suspend(())?;
        }
    }
}

struct Reset<T: Copy>(*mut T, T);

impl<T: Copy> Drop for Reset<T> {
    fn drop(&mut self) {
        unsafe {
            *self.0 = self.1;
        }
    }
}
