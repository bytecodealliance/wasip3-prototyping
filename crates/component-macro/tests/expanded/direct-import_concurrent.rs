/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `foo`.
///
/// This structure is created through [`FooPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`Foo`] as well.
pub struct FooPre<T> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: FooIndices,
}
impl<T> Clone for FooPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T> FooPre<_T> {
    /// Creates a new copy of `FooPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = FooIndices::new(&instance_pre)?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`Foo`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<Foo>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `foo`.
///
/// This is an implementation detail of [`FooPre`] and can
/// be constructed if needed as well.
///
/// For more information see [`Foo`] as well.
#[derive(Clone)]
pub struct FooIndices {}
/// Auto-generated bindings for an instance a component which
/// implements the world `foo`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`Foo::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`FooPre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`FooPre::instantiate_async`] to
///   create a [`Foo`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`Foo::new`].
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct Foo {}
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait FooImports: Send {
    fn foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<Output = ()> + Send
    where
        Self: Sized;
}
struct State {
    host: *mut u8,
    store: *mut u8,
    spawned: Vec<wasmtime::component::__internal::Spawned>,
}
thread_local! {
    static STATE : wasmtime::component::__internal::RefCell < Option < State >> =
    wasmtime::component::__internal::RefCell::new(None);
}
struct ResetState(Option<State>);
impl Drop for ResetState {
    fn drop(&mut self) {
        STATE
            .with(|v| {
                *v.borrow_mut() = self.0.take();
            })
    }
}
fn get_host_and_store() -> (*mut u8, *mut u8) {
    STATE
        .with(|v| v.borrow().as_ref().map(|State { host, store, .. }| (*host, *store)))
        .unwrap()
}
fn spawn_task(task: wasmtime::component::__internal::Spawned) {
    STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
}
fn poll_with_state<
    T,
    G: for<'a> wasmtime::component::GetHost<&'a mut T>,
    F: wasmtime::component::__internal::Future + ?Sized,
>(
    getter: G,
    store: wasmtime::VMStoreRawPtr,
    instance: Option<wasmtime::component::Instance>,
    cx: &mut wasmtime::component::__internal::Context,
    future: wasmtime::component::__internal::Pin<&mut F>,
) -> wasmtime::component::__internal::Poll<F::Output> {
    use wasmtime::component::__internal::{AbortWrapper, mem, DerefMut, Poll};
    let mut store_cx = unsafe {
        wasmtime::StoreContextMut::new(&mut *store.0.as_ptr().cast())
    };
    let (result, spawned) = {
        let host = &mut getter(store_cx.data_mut());
        let old = STATE
            .with(|v| {
                v
                    .replace(
                        Some(State {
                            host: (host as *mut G::Host).cast(),
                            store: store.0.as_ptr().cast(),
                            spawned: Vec::new(),
                        }),
                    )
            });
        let _reset = ResetState(old);
        (future.poll(cx), STATE.with(|v| v.take()).unwrap().spawned)
    };
    for spawned in spawned {
        instance
            .unwrap()
            .spawn_raw(
                &mut store_cx,
                wasmtime::component::__internal::poll_fn(move |cx| {
                    let mut spawned = spawned.try_lock().unwrap();
                    let inner = mem::replace(
                        DerefMut::deref_mut(&mut spawned),
                        AbortWrapper::Aborted,
                    );
                    if let AbortWrapper::Unpolled(mut future)
                    | AbortWrapper::Polled { mut future, .. } = inner {
                        let result = poll_with_state(
                            getter,
                            store,
                            instance,
                            cx,
                            future.as_mut(),
                        );
                        *DerefMut::deref_mut(&mut spawned) = AbortWrapper::Polled {
                            future,
                            waker: cx.waker().clone(),
                        };
                        result
                    } else {
                        Poll::Ready(Ok(()))
                    }
                }),
            )
    }
    result
}
impl<_T: FooImports + Send> FooImports for &mut _T {
    async fn foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> () {
        struct Task {}
        impl<T: 'static, U: FooImports> wasmtime::component::AccessorTask<T, U, ()>
        for Task {
            async fn run(
                self,
                accessor: &mut wasmtime::component::Accessor<T, U>,
            ) -> () {
                <U as FooImports>::foo(accessor).await
            }
        }
        accessor.forward(|v| *v, Task {}).await
    }
}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl FooIndices {
        /// Creates a new copy of `FooIndices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new<_T>(
            _instance_pre: &wasmtime::component::InstancePre<_T>,
        ) -> wasmtime::Result<Self> {
            let _component = _instance_pre.component();
            let _instance_type = _instance_pre.instance_type();
            Ok(FooIndices {})
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`Foo`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Foo> {
            let _ = &mut store;
            let _instance = instance;
            Ok(Foo {})
        }
    }
    impl Foo {
        /// Convenience wrapper around [`FooPre::new`] and
        /// [`FooPre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<Foo>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            FooPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`FooIndices::new`] and
        /// [`FooIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Foo> {
            let indices = FooIndices::new(&instance.instance_pre(&store))?;
            indices.load(&mut store, instance)
        }
        pub fn add_to_linker_imports_get_host<T, G>(
            linker: &mut wasmtime::component::Linker<T>,
            host_getter: G,
        ) -> wasmtime::Result<()>
        where
            G: for<'a> wasmtime::component::GetHost<&'a mut T, Host: FooImports>,
            T: Send + 'static,
        {
            let mut linker = linker.root();
            linker
                .func_wrap_concurrent(
                    "foo",
                    move |caller: &mut wasmtime::component::Accessor<T, T>, (): ()| {
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::<
                                T,
                                _,
                            >::new(
                                get_host_and_store,
                                spawn_task,
                                caller.maybe_instance(),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as FooImports>::foo(&mut accessor).await;
                            Ok(r)
                        });
                        let store = wasmtime::VMStoreRawPtr(
                            caller
                                .with(|mut v| {
                                    wasmtime::AsContextMut::as_context_mut(&mut v).traitobj()
                                }),
                        );
                        let instance = caller.maybe_instance();
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| {
                                poll_with_state(
                                    host_getter,
                                    store,
                                    instance,
                                    cx,
                                    future.as_mut(),
                                )
                            }),
                        )
                    },
                )?;
            Ok(())
        }
        pub fn add_to_linker<T, U>(
            linker: &mut wasmtime::component::Linker<T>,
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
            U: FooImports + Send,
        {
            Self::add_to_linker_imports_get_host(linker, get)?;
            Ok(())
        }
    }
};
