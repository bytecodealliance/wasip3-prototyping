/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `d`.
///
/// This structure is created through [`DPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`D`] as well.
pub struct DPre<T> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: DIndices,
}
impl<T> Clone for DPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T> DPre<_T> {
    /// Creates a new copy of `DPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = DIndices::new(instance_pre.component())?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`D`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<D>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `d`.
///
/// This is an implementation detail of [`DPre`] and can
/// be constructed if needed as well.
///
/// For more information see [`D`] as well.
#[derive(Clone)]
pub struct DIndices {}
/// Auto-generated bindings for an instance a component which
/// implements the world `d`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`D::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`DPre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`DPre::instantiate_async`] to
///   create a [`D`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`D::new`].
///
/// * You can also access the guts of instantiation through
///   [`DIndices::new_instance`] followed
///   by [`DIndices::load`] to crate an instance of this
///   type.
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct D {}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl DIndices {
        /// Creates a new copy of `DIndices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new(
            component: &wasmtime::component::Component,
        ) -> wasmtime::Result<Self> {
            let _component = component;
            Ok(DIndices {})
        }
        /// Creates a new instance of [`DIndices`] from an
        /// instantiated component.
        ///
        /// This method of creating a [`D`] will perform string
        /// lookups for all exports when this method is called. This
        /// will only succeed if the provided instance matches the
        /// requirements of [`D`].
        pub fn new_instance(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Self> {
            let _instance = instance;
            Ok(DIndices {})
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`D`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<D> {
            let _instance = instance;
            Ok(D {})
        }
    }
    impl D {
        /// Convenience wrapper around [`DPre::new`] and
        /// [`DPre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            mut store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<D>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            DPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`DIndices::new_instance`] and
        /// [`DIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<D> {
            let indices = DIndices::new_instance(&mut store, instance)?;
            indices.load(store, instance)
        }
        pub fn add_to_linker<T, U>(
            linker: &mut wasmtime::component::Linker<T>,
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
            U: foo::foo::a::Host + foo::foo::b::Host + foo::foo::c::Host + d::Host
                + Send,
        {
            foo::foo::a::add_to_linker(linker, get)?;
            foo::foo::b::add_to_linker(linker, get)?;
            foo::foo::c::add_to_linker(linker, get)?;
            d::add_to_linker(linker, get)?;
            Ok(())
        }
    }
};
pub mod foo {
    pub mod foo {
        #[allow(clippy::all)]
        pub mod a {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            #[derive(wasmtime::component::ComponentType)]
            #[derive(wasmtime::component::Lift)]
            #[derive(wasmtime::component::Lower)]
            #[component(record)]
            #[derive(Clone, Copy)]
            pub struct Foo {}
            impl core::fmt::Debug for Foo {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.debug_struct("Foo").finish()
                }
            }
            const _: () = {
                assert!(0 == < Foo as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Foo as wasmtime::component::ComponentType >::ALIGN32);
            };
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Foo> + Send
                where
                    Self: Sized;
            }
            struct State {
                host: *mut u8,
                store: *mut u8,
                spawned: Vec<wasmtime::component::__internal::Spawned>,
            }
            thread_local! {
                static STATE : wasmtime::component::__internal::RefCell < Option < State
                >> = wasmtime::component::__internal::RefCell::new(None);
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
                    .with(|v| {
                        v
                            .borrow()
                            .as_ref()
                            .map(|State { host, store, .. }| (*host, *store))
                    })
                    .unwrap()
            }
            fn spawn_task(task: wasmtime::component::__internal::Spawned) {
                STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
            }
            fn poll_with_state<
                T,
                G: for<'a> GetHost<&'a mut T>,
                F: wasmtime::component::__internal::Future + ?Sized,
            >(
                getter: G,
                store: wasmtime::VMStoreRawPtr,
                instance: Option<wasmtime::component::Instance>,
                cx: &mut wasmtime::component::__internal::Context,
                future: wasmtime::component::__internal::Pin<&mut F>,
            ) -> wasmtime::component::__internal::Poll<F::Output> {
                use wasmtime::component::__internal::{SpawnedInner, mem, DerefMut, Poll};
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
                        .spawn(
                            &mut store_cx,
                            wasmtime::component::__internal::poll_fn(move |cx| {
                                let mut spawned = spawned.try_lock().unwrap();
                                let inner = mem::replace(
                                    DerefMut::deref_mut(&mut spawned),
                                    SpawnedInner::Aborted,
                                );
                                if let SpawnedInner::Unpolled(mut future)
                                | SpawnedInner::Polled { mut future, .. } = inner {
                                    let result = poll_with_state(
                                        getter,
                                        store,
                                        instance,
                                        cx,
                                        future.as_mut(),
                                    );
                                    *DerefMut::deref_mut(&mut spawned) = SpawnedInner::Polled {
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
            pub trait GetHost<
                T,
            >: Fn(T) -> <Self as GetHost<T>>::Host + Send + Sync + Copy + 'static {
                type Host: Host + Send;
            }
            impl<F, T, O> GetHost<T> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, Host: Host + Send>,
            >(
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: G,
            ) -> wasmtime::Result<()>
            where
                T: Send + 'static,
            {
                let mut inst = linker.instance("foo:foo/a")?;
                inst.func_wrap_concurrent(
                    "a",
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
                            let r = <G::Host as Host>::a(&mut accessor).await;
                            Ok((r,))
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
                U: Host + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host + Send> Host for &mut _T {
                async fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> Foo {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Foo> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Foo {
                            <U as Host>::a(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
            }
        }
        #[allow(clippy::all)]
        pub mod b {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub type Foo = super::super::super::foo::foo::a::Foo;
            const _: () = {
                assert!(0 == < Foo as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Foo as wasmtime::component::ComponentType >::ALIGN32);
            };
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Foo> + Send
                where
                    Self: Sized;
            }
            struct State {
                host: *mut u8,
                store: *mut u8,
                spawned: Vec<wasmtime::component::__internal::Spawned>,
            }
            thread_local! {
                static STATE : wasmtime::component::__internal::RefCell < Option < State
                >> = wasmtime::component::__internal::RefCell::new(None);
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
                    .with(|v| {
                        v
                            .borrow()
                            .as_ref()
                            .map(|State { host, store, .. }| (*host, *store))
                    })
                    .unwrap()
            }
            fn spawn_task(task: wasmtime::component::__internal::Spawned) {
                STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
            }
            fn poll_with_state<
                T,
                G: for<'a> GetHost<&'a mut T>,
                F: wasmtime::component::__internal::Future + ?Sized,
            >(
                getter: G,
                store: wasmtime::VMStoreRawPtr,
                instance: Option<wasmtime::component::Instance>,
                cx: &mut wasmtime::component::__internal::Context,
                future: wasmtime::component::__internal::Pin<&mut F>,
            ) -> wasmtime::component::__internal::Poll<F::Output> {
                use wasmtime::component::__internal::{SpawnedInner, mem, DerefMut, Poll};
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
                        .spawn(
                            &mut store_cx,
                            wasmtime::component::__internal::poll_fn(move |cx| {
                                let mut spawned = spawned.try_lock().unwrap();
                                let inner = mem::replace(
                                    DerefMut::deref_mut(&mut spawned),
                                    SpawnedInner::Aborted,
                                );
                                if let SpawnedInner::Unpolled(mut future)
                                | SpawnedInner::Polled { mut future, .. } = inner {
                                    let result = poll_with_state(
                                        getter,
                                        store,
                                        instance,
                                        cx,
                                        future.as_mut(),
                                    );
                                    *DerefMut::deref_mut(&mut spawned) = SpawnedInner::Polled {
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
            pub trait GetHost<
                T,
            >: Fn(T) -> <Self as GetHost<T>>::Host + Send + Sync + Copy + 'static {
                type Host: Host + Send;
            }
            impl<F, T, O> GetHost<T> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, Host: Host + Send>,
            >(
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: G,
            ) -> wasmtime::Result<()>
            where
                T: Send + 'static,
            {
                let mut inst = linker.instance("foo:foo/b")?;
                inst.func_wrap_concurrent(
                    "a",
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
                            let r = <G::Host as Host>::a(&mut accessor).await;
                            Ok((r,))
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
                U: Host + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host + Send> Host for &mut _T {
                async fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> Foo {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Foo> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Foo {
                            <U as Host>::a(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
            }
        }
        #[allow(clippy::all)]
        pub mod c {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub type Foo = super::super::super::foo::foo::b::Foo;
            const _: () = {
                assert!(0 == < Foo as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Foo as wasmtime::component::ComponentType >::ALIGN32);
            };
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Foo> + Send
                where
                    Self: Sized;
            }
            struct State {
                host: *mut u8,
                store: *mut u8,
                spawned: Vec<wasmtime::component::__internal::Spawned>,
            }
            thread_local! {
                static STATE : wasmtime::component::__internal::RefCell < Option < State
                >> = wasmtime::component::__internal::RefCell::new(None);
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
                    .with(|v| {
                        v
                            .borrow()
                            .as_ref()
                            .map(|State { host, store, .. }| (*host, *store))
                    })
                    .unwrap()
            }
            fn spawn_task(task: wasmtime::component::__internal::Spawned) {
                STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
            }
            fn poll_with_state<
                T,
                G: for<'a> GetHost<&'a mut T>,
                F: wasmtime::component::__internal::Future + ?Sized,
            >(
                getter: G,
                store: wasmtime::VMStoreRawPtr,
                instance: Option<wasmtime::component::Instance>,
                cx: &mut wasmtime::component::__internal::Context,
                future: wasmtime::component::__internal::Pin<&mut F>,
            ) -> wasmtime::component::__internal::Poll<F::Output> {
                use wasmtime::component::__internal::{SpawnedInner, mem, DerefMut, Poll};
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
                        .spawn(
                            &mut store_cx,
                            wasmtime::component::__internal::poll_fn(move |cx| {
                                let mut spawned = spawned.try_lock().unwrap();
                                let inner = mem::replace(
                                    DerefMut::deref_mut(&mut spawned),
                                    SpawnedInner::Aborted,
                                );
                                if let SpawnedInner::Unpolled(mut future)
                                | SpawnedInner::Polled { mut future, .. } = inner {
                                    let result = poll_with_state(
                                        getter,
                                        store,
                                        instance,
                                        cx,
                                        future.as_mut(),
                                    );
                                    *DerefMut::deref_mut(&mut spawned) = SpawnedInner::Polled {
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
            pub trait GetHost<
                T,
            >: Fn(T) -> <Self as GetHost<T>>::Host + Send + Sync + Copy + 'static {
                type Host: Host + Send;
            }
            impl<F, T, O> GetHost<T> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, Host: Host + Send>,
            >(
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: G,
            ) -> wasmtime::Result<()>
            where
                T: Send + 'static,
            {
                let mut inst = linker.instance("foo:foo/c")?;
                inst.func_wrap_concurrent(
                    "a",
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
                            let r = <G::Host as Host>::a(&mut accessor).await;
                            Ok((r,))
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
                U: Host + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host + Send> Host for &mut _T {
                async fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> Foo {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Foo> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Foo {
                            <U as Host>::a(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
            }
        }
    }
}
#[allow(clippy::all)]
pub mod d {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::{anyhow, Box};
    pub type Foo = super::foo::foo::c::Foo;
    const _: () = {
        assert!(0 == < Foo as wasmtime::component::ComponentType >::SIZE32);
        assert!(1 == < Foo as wasmtime::component::ComponentType >::ALIGN32);
    };
    #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
    pub trait Host: Send {
        fn b<T: 'static>(
            accessor: &mut wasmtime::component::Accessor<T, Self>,
        ) -> impl ::core::future::Future<Output = Foo> + Send
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
            .with(|v| {
                v.borrow().as_ref().map(|State { host, store, .. }| (*host, *store))
            })
            .unwrap()
    }
    fn spawn_task(task: wasmtime::component::__internal::Spawned) {
        STATE.with(|v| v.borrow_mut().as_mut().unwrap().spawned.push(task));
    }
    fn poll_with_state<
        T,
        G: for<'a> GetHost<&'a mut T>,
        F: wasmtime::component::__internal::Future + ?Sized,
    >(
        getter: G,
        store: wasmtime::VMStoreRawPtr,
        instance: Option<wasmtime::component::Instance>,
        cx: &mut wasmtime::component::__internal::Context,
        future: wasmtime::component::__internal::Pin<&mut F>,
    ) -> wasmtime::component::__internal::Poll<F::Output> {
        use wasmtime::component::__internal::{SpawnedInner, mem, DerefMut, Poll};
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
                .spawn(
                    &mut store_cx,
                    wasmtime::component::__internal::poll_fn(move |cx| {
                        let mut spawned = spawned.try_lock().unwrap();
                        let inner = mem::replace(
                            DerefMut::deref_mut(&mut spawned),
                            SpawnedInner::Aborted,
                        );
                        if let SpawnedInner::Unpolled(mut future)
                        | SpawnedInner::Polled { mut future, .. } = inner {
                            let result = poll_with_state(
                                getter,
                                store,
                                instance,
                                cx,
                                future.as_mut(),
                            );
                            *DerefMut::deref_mut(&mut spawned) = SpawnedInner::Polled {
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
    pub trait GetHost<
        T,
    >: Fn(T) -> <Self as GetHost<T>>::Host + Send + Sync + Copy + 'static {
        type Host: Host + Send;
    }
    impl<F, T, O> GetHost<T> for F
    where
        F: Fn(T) -> O + Send + Sync + Copy + 'static,
        O: Host + Send,
    {
        type Host = O;
    }
    pub fn add_to_linker_get_host<T, G: for<'a> GetHost<&'a mut T, Host: Host + Send>>(
        linker: &mut wasmtime::component::Linker<T>,
        host_getter: G,
    ) -> wasmtime::Result<()>
    where
        T: Send + 'static,
    {
        let mut inst = linker.instance("d")?;
        inst.func_wrap_concurrent(
            "b",
            move |caller: &mut wasmtime::component::Accessor<T, T>, (): ()| {
                let mut accessor = unsafe {
                    wasmtime::component::Accessor::<
                        T,
                        _,
                    >::new(get_host_and_store, spawn_task, caller.maybe_instance())
                };
                let mut future = wasmtime::component::__internal::Box::pin(async move {
                    let r = <G::Host as Host>::b(&mut accessor).await;
                    Ok((r,))
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
        U: Host + Send,
        T: Send + 'static,
    {
        add_to_linker_get_host(linker, get)
    }
    impl<_T: Host + Send> Host for &mut _T {
        async fn b<T: 'static>(
            accessor: &mut wasmtime::component::Accessor<T, Self>,
        ) -> Foo {
            struct Task {}
            impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, Foo>
            for Task {
                async fn run(
                    self,
                    accessor: &mut wasmtime::component::Accessor<T, U>,
                ) -> Foo {
                    <U as Host>::b(accessor).await
                }
            }
            accessor.forward(|v| *v, Task {}).await
        }
    }
}
