pub enum WorldResource {}
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait HostWorldResource: Sized {
    fn new<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<
        Output = wasmtime::component::Resource<WorldResource>,
    > + Send
    where
        Self: Sized;
    fn foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
        self_: wasmtime::component::Resource<WorldResource>,
    ) -> impl ::core::future::Future<Output = ()> + Send
    where
        Self: Sized;
    fn static_foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<Output = ()> + Send
    where
        Self: Sized;
    async fn drop(
        &mut self,
        rep: wasmtime::component::Resource<WorldResource>,
    ) -> wasmtime::Result<()>;
}
impl<_T: HostWorldResource> HostWorldResource for &mut _T {
    async fn new<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> wasmtime::component::Resource<WorldResource> {
        struct Task {}
        impl<
            T: 'static,
            U: HostWorldResource,
        > wasmtime::component::AccessorTask<
            T,
            U,
            wasmtime::component::Resource<WorldResource>,
        > for Task {
            async fn run(
                self,
                accessor: &mut wasmtime::component::Accessor<T, U>,
            ) -> wasmtime::component::Resource<WorldResource> {
                <U as HostWorldResource>::new(accessor).await
            }
        }
        accessor.forward(|v| *v, Task {}).await
    }
    async fn foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
        self_: wasmtime::component::Resource<WorldResource>,
    ) -> () {
        struct Task {
            self_: wasmtime::component::Resource<WorldResource>,
        }
        impl<
            T: 'static,
            U: HostWorldResource,
        > wasmtime::component::AccessorTask<T, U, ()> for Task {
            async fn run(
                self,
                accessor: &mut wasmtime::component::Accessor<T, U>,
            ) -> () {
                <U as HostWorldResource>::foo(accessor, self.self_).await
            }
        }
        accessor.forward(|v| *v, Task { self_ }).await
    }
    async fn static_foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> () {
        struct Task {}
        impl<
            T: 'static,
            U: HostWorldResource,
        > wasmtime::component::AccessorTask<T, U, ()> for Task {
            async fn run(
                self,
                accessor: &mut wasmtime::component::Accessor<T, U>,
            ) -> () {
                <U as HostWorldResource>::static_foo(accessor).await
            }
        }
        accessor.forward(|v| *v, Task {}).await
    }
    async fn drop(
        &mut self,
        rep: wasmtime::component::Resource<WorldResource>,
    ) -> wasmtime::Result<()> {
        HostWorldResource::drop(*self, rep).await
    }
}
/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `the-world`.
///
/// This structure is created through [`TheWorldPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`TheWorld`] as well.
pub struct TheWorldPre<T> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: TheWorldIndices,
}
impl<T> Clone for TheWorldPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T> TheWorldPre<_T> {
    /// Creates a new copy of `TheWorldPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = TheWorldIndices::new(&instance_pre)?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`TheWorld`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<TheWorld>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `the-world`.
///
/// This is an implementation detail of [`TheWorldPre`] and can
/// be constructed if needed as well.
///
/// For more information see [`TheWorld`] as well.
#[derive(Clone)]
pub struct TheWorldIndices {
    interface1: exports::foo::foo::uses_resource_transitively::GuestIndices,
    some_world_func2: wasmtime::component::ComponentExportIndex,
}
/// Auto-generated bindings for an instance a component which
/// implements the world `the-world`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`TheWorld::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`TheWorldPre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`TheWorldPre::instantiate_async`] to
///   create a [`TheWorld`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`TheWorld::new`].
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct TheWorld {
    interface1: exports::foo::foo::uses_resource_transitively::Guest,
    some_world_func2: wasmtime::component::Func,
}
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait TheWorldImports: Send + HostWorldResource {
    fn some_world_func<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<
        Output = wasmtime::component::Resource<WorldResource>,
    > + Send
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
    G: for<'a> TheWorldImportsGetHost<&'a mut T>,
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
pub trait TheWorldImportsGetHost<
    T,
>: Fn(T) -> <Self as TheWorldImportsGetHost<T>>::Host + Send + Sync + Copy + 'static {
    type Host: TheWorldImports;
}
impl<F, T, O> TheWorldImportsGetHost<T> for F
where
    F: Fn(T) -> O + Send + Sync + Copy + 'static,
    O: TheWorldImports,
{
    type Host = O;
}
impl<_T: TheWorldImports + Send> TheWorldImports for &mut _T {
    async fn some_world_func<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> wasmtime::component::Resource<WorldResource> {
        struct Task {}
        impl<
            T: 'static,
            U: TheWorldImports,
        > wasmtime::component::AccessorTask<
            T,
            U,
            wasmtime::component::Resource<WorldResource>,
        > for Task {
            async fn run(
                self,
                accessor: &mut wasmtime::component::Accessor<T, U>,
            ) -> wasmtime::component::Resource<WorldResource> {
                <U as TheWorldImports>::some_world_func(accessor).await
            }
        }
        accessor.forward(|v| *v, Task {}).await
    }
}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl TheWorldIndices {
        /// Creates a new copy of `TheWorldIndices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new<_T>(
            _instance_pre: &wasmtime::component::InstancePre<_T>,
        ) -> wasmtime::Result<Self> {
            let _component = _instance_pre.component();
            let _instance_type = _instance_pre.instance_type();
            let interface1 = exports::foo::foo::uses_resource_transitively::GuestIndices::new(
                _instance_pre,
            )?;
            let some_world_func2 = {
                let (item, index) = _component
                    .get_export(None, "some-world-func2")
                    .ok_or_else(|| {
                        anyhow::anyhow!("no export `some-world-func2` found")
                    })?;
                match item {
                    wasmtime::component::types::ComponentItem::ComponentFunc(func) => {
                        anyhow::Context::context(
                            func
                                .typecheck::<
                                    (),
                                    (wasmtime::component::Resource<WorldResource>,),
                                >(&_instance_type),
                            "type-checking export func `some-world-func2`",
                        )?;
                        index
                    }
                    _ => {
                        Err(
                            anyhow::anyhow!(
                                "export `some-world-func2` is not a function"
                            ),
                        )?
                    }
                }
            };
            Ok(TheWorldIndices {
                interface1,
                some_world_func2,
            })
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`TheWorld`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<TheWorld> {
            let _ = &mut store;
            let _instance = instance;
            let interface1 = self.interface1.load(&mut store, &_instance)?;
            let some_world_func2 = *_instance
                .get_typed_func::<
                    (),
                    (wasmtime::component::Resource<WorldResource>,),
                >(&mut store, &self.some_world_func2)?
                .func();
            Ok(TheWorld {
                interface1,
                some_world_func2,
            })
        }
    }
    impl TheWorld {
        /// Convenience wrapper around [`TheWorldPre::new`] and
        /// [`TheWorldPre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<TheWorld>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            TheWorldPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`TheWorldIndices::new`] and
        /// [`TheWorldIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<TheWorld> {
            let indices = TheWorldIndices::new(&instance.instance_pre(&store))?;
            indices.load(&mut store, instance)
        }
        pub fn add_to_linker_imports_get_host<
            T,
            G: for<'a> TheWorldImportsGetHost<&'a mut T, Host: TheWorldImports>,
        >(
            linker: &mut wasmtime::component::Linker<T>,
            host_getter: G,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
        {
            let mut linker = linker.root();
            linker
                .resource_async(
                    "world-resource",
                    wasmtime::component::ResourceType::host::<WorldResource>(),
                    move |mut store, rep| {
                        wasmtime::component::__internal::Box::new(async move {
                            HostWorldResource::drop(
                                    &mut host_getter(store.data_mut()),
                                    wasmtime::component::Resource::new_own(rep),
                                )
                                .await
                        })
                    },
                )?;
            linker
                .func_wrap_concurrent(
                    "some-world-func",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        let host = caller;
                        let r = <G::Host as TheWorldImports>::some_world_func(host);
                        Box::pin(async move {
                            let fun = r.await;
                            Box::new(move |mut caller: wasmtime::StoreContextMut<'_, T>| {
                                let r = fun(caller);
                                Ok((r,))
                            })
                                as Box<
                                    dyn FnOnce(
                                        wasmtime::StoreContextMut<'_, T>,
                                    ) -> wasmtime::Result<
                                            (wasmtime::component::Resource<WorldResource>,),
                                        > + Send + Sync,
                                >
                        })
                            as ::core::pin::Pin<
                                Box<
                                    dyn ::core::future::Future<
                                        Output = Box<
                                            dyn FnOnce(
                                                wasmtime::StoreContextMut<'_, T>,
                                            ) -> wasmtime::Result<
                                                    (wasmtime::component::Resource<WorldResource>,),
                                                > + Send + Sync,
                                        >,
                                    > + Send + Sync + 'static,
                                >,
                            >
                    },
                )?;
            linker
                .func_wrap_concurrent(
                    "[constructor]world-resource",
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
                            let r = <G::Host as HostWorldResource>::new(&mut accessor)
                                .await;
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
            linker
                .func_wrap_concurrent(
                    "[method]world-resource.foo",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (wasmtime::component::Resource<WorldResource>,)|
                    {
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
                            let r = <G::Host as HostWorldResource>::foo(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
            linker
                .func_wrap_concurrent(
                    "[static]world-resource.static-foo",
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
                            let r = <G::Host as HostWorldResource>::static_foo(
                                    &mut accessor,
                                )
                                .await;
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
<<<<<<< HEAD
            linker
                .func_wrap_concurrent(
                    "some-world-func",
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
                            let r = <G::Host as TheWorldImports>::some_world_func(
                                    &mut accessor,
                                )
                                .await;
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
||||||| d48b3856b
            linker
                .func_wrap_concurrent(
                    "some-world-func",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        let host = caller;
                        let r = <G::Host as TheWorldImports>::some_world_func(host);
                        Box::pin(async move {
                            let fun = r.await;
                            Box::new(move |mut caller: wasmtime::StoreContextMut<'_, T>| {
                                let r = fun(caller);
                                Ok((r,))
                            })
                                as Box<
                                    dyn FnOnce(
                                        wasmtime::StoreContextMut<'_, T>,
                                    ) -> wasmtime::Result<
                                            (wasmtime::component::Resource<WorldResource>,),
                                        > + Send + Sync,
                                >
                        })
                            as ::core::pin::Pin<
                                Box<
                                    dyn ::core::future::Future<
                                        Output = Box<
                                            dyn FnOnce(
                                                wasmtime::StoreContextMut<'_, T>,
                                            ) -> wasmtime::Result<
                                                    (wasmtime::component::Resource<WorldResource>,),
                                                > + Send + Sync,
                                        >,
                                    > + Send + Sync + 'static,
                                >,
                            >
                    },
                )?;
=======
>>>>>>> upstream/main
            Ok(())
        }
        pub fn add_to_linker<T, U>(
            linker: &mut wasmtime::component::Linker<T>,
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
            U: foo::foo::resources::Host + foo::foo::long_use_chain1::Host
                + foo::foo::long_use_chain2::Host + foo::foo::long_use_chain3::Host
                + foo::foo::long_use_chain4::Host
                + foo::foo::transitive_interface_with_resource::Host + TheWorldImports
                + Send,
        {
            Self::add_to_linker_imports_get_host(linker, get)?;
            foo::foo::resources::add_to_linker(linker, get)?;
            foo::foo::long_use_chain1::add_to_linker(linker, get)?;
            foo::foo::long_use_chain2::add_to_linker(linker, get)?;
            foo::foo::long_use_chain3::add_to_linker(linker, get)?;
            foo::foo::long_use_chain4::add_to_linker(linker, get)?;
            foo::foo::transitive_interface_with_resource::add_to_linker(linker, get)?;
            Ok(())
        }
        pub fn call_some_world_func2<S: wasmtime::AsContextMut>(
            &self,
            mut store: S,
        ) -> impl wasmtime::component::__internal::Future<
            Output = wasmtime::Result<wasmtime::component::Resource<WorldResource>>,
        > + Send + 'static + use<S>
        where
            <S as wasmtime::AsContext>::Data: Send,
        {
            let callee = unsafe {
                wasmtime::component::TypedFunc::<
                    (),
                    (wasmtime::component::Resource<WorldResource>,),
                >::new_unchecked(self.some_world_func2)
            };
            wasmtime::component::__internal::FutureExt::map(
                callee.call_concurrent(store.as_context_mut(), ()),
                |v| v.map(|(v,)| v),
            )
        }
        pub fn foo_foo_uses_resource_transitively(
            &self,
        ) -> &exports::foo::foo::uses_resource_transitively::Guest {
            &self.interface1
        }
    }
};
pub mod foo {
    pub mod foo {
        #[allow(clippy::all)]
        pub mod resources {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub enum Bar {}
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait HostBar: Sized {
                fn new<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::Resource<Bar>,
                > + Send
                where
                    Self: Sized;
                fn static_a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = u32> + Send
                where
                    Self: Sized;
                fn method_a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    self_: wasmtime::component::Resource<Bar>,
                ) -> impl ::core::future::Future<Output = u32> + Send
                where
                    Self: Sized;
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<Bar>,
                ) -> wasmtime::Result<()>;
            }
            impl<_T: HostBar> HostBar for &mut _T {
                async fn new<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> wasmtime::component::Resource<Bar> {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: HostBar,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::Resource<Bar>,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::Resource<Bar> {
                            <U as HostBar>::new(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn static_a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> u32 {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: HostBar,
                    > wasmtime::component::AccessorTask<T, U, u32> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> u32 {
                            <U as HostBar>::static_a(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn method_a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    self_: wasmtime::component::Resource<Bar>,
                ) -> u32 {
                    struct Task {
                        self_: wasmtime::component::Resource<Bar>,
                    }
                    impl<
                        T: 'static,
                        U: HostBar,
                    > wasmtime::component::AccessorTask<T, U, u32> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> u32 {
                            <U as HostBar>::method_a(accessor, self.self_).await
                        }
                    }
                    accessor.forward(|v| *v, Task { self_ }).await
                }
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<Bar>,
                ) -> wasmtime::Result<()> {
                    HostBar::drop(*self, rep).await
                }
            }
            #[derive(wasmtime::component::ComponentType)]
            #[derive(wasmtime::component::Lift)]
            #[derive(wasmtime::component::Lower)]
            #[component(record)]
            pub struct NestedOwn {
                #[component(name = "nested-bar")]
                pub nested_bar: wasmtime::component::Resource<Bar>,
            }
            impl core::fmt::Debug for NestedOwn {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.debug_struct("NestedOwn")
                        .field("nested-bar", &self.nested_bar)
                        .finish()
                }
            }
            const _: () = {
                assert!(
                    4 == < NestedOwn as wasmtime::component::ComponentType >::SIZE32
                );
                assert!(
                    4 == < NestedOwn as wasmtime::component::ComponentType >::ALIGN32
                );
            };
            #[derive(wasmtime::component::ComponentType)]
            #[derive(wasmtime::component::Lift)]
            #[derive(wasmtime::component::Lower)]
            #[component(record)]
            pub struct NestedBorrow {
                #[component(name = "nested-bar")]
                pub nested_bar: wasmtime::component::Resource<Bar>,
            }
            impl core::fmt::Debug for NestedBorrow {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.debug_struct("NestedBorrow")
                        .field("nested-bar", &self.nested_bar)
                        .finish()
                }
            }
            const _: () = {
                assert!(
                    4 == < NestedBorrow as wasmtime::component::ComponentType >::SIZE32
                );
                assert!(
                    4 == < NestedBorrow as wasmtime::component::ComponentType >::ALIGN32
                );
            };
            pub type SomeHandle = wasmtime::component::Resource<Bar>;
            const _: () = {
                assert!(
                    4 == < SomeHandle as wasmtime::component::ComponentType >::SIZE32
                );
                assert!(
                    4 == < SomeHandle as wasmtime::component::ComponentType >::ALIGN32
                );
            };
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send + HostBar + Sized {
                fn bar_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::Resource<Bar>,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn bar_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::Resource<Bar>,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn bar_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::Resource<Bar>,
                > + Send
                where
                    Self: Sized;
                fn tuple_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: (wasmtime::component::Resource<Bar>, u32),
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn tuple_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: (wasmtime::component::Resource<Bar>, u32),
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn tuple_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = (wasmtime::component::Resource<Bar>, u32),
                > + Send
                where
                    Self: Sized;
                fn option_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Option<wasmtime::component::Resource<Bar>>,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn option_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Option<wasmtime::component::Resource<Bar>>,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn option_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = Option<wasmtime::component::Resource<Bar>>,
                > + Send
                where
                    Self: Sized;
                fn result_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Result<wasmtime::component::Resource<Bar>, ()>,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn result_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Result<wasmtime::component::Resource<Bar>, ()>,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn result_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = Result<wasmtime::component::Resource<Bar>, ()>,
                > + Send
                where
                    Self: Sized;
                fn list_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn list_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn list_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                > + Send
                where
                    Self: Sized;
                fn record_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: NestedOwn,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn record_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: NestedBorrow,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn record_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = NestedOwn> + Send
                where
                    Self: Sized;
                fn func_with_handle_typedef<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: SomeHandle,
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
                let mut inst = linker.instance("foo:foo/resources")?;
                inst.resource_async(
                    "bar",
                    wasmtime::component::ResourceType::host::<Bar>(),
                    move |mut store, rep| {
                        wasmtime::component::__internal::Box::new(async move {
                            HostBar::drop(
                                    &mut host_getter(store.data_mut()),
                                    wasmtime::component::Resource::new_own(rep),
                                )
                                .await
                        })
                    },
                )?;
                inst.func_wrap_concurrent(
                    "[constructor]bar",
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
                            let r = <G::Host as HostBar>::new(&mut accessor).await;
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
                inst.func_wrap_concurrent(
                    "[static]bar.static-a",
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
                            let r = <G::Host as HostBar>::static_a(&mut accessor).await;
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
                inst.func_wrap_concurrent(
                    "[method]bar.method-a",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (wasmtime::component::Resource<Bar>,)|
                    {
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
                            let r = <G::Host as HostBar>::method_a(&mut accessor, arg0)
                                .await;
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
                inst.func_wrap_concurrent(
                    "bar-own-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (wasmtime::component::Resource<Bar>,)|
                    {
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
                            let r = <G::Host as Host>::bar_own_arg(&mut accessor, arg0)
                                .await;
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
                inst.func_wrap_concurrent(
                    "bar-borrow-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (wasmtime::component::Resource<Bar>,)|
                    {
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
                            let r = <G::Host as Host>::bar_borrow_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "bar-result",
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
                            let r = <G::Host as Host>::bar_result(&mut accessor).await;
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
                inst.func_wrap_concurrent(
                    "tuple-own-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): ((wasmtime::component::Resource<Bar>, u32),)|
                    {
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
                            let r = <G::Host as Host>::tuple_own_arg(&mut accessor, arg0)
                                .await;
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
                inst.func_wrap_concurrent(
                    "tuple-borrow-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): ((wasmtime::component::Resource<Bar>, u32),)|
                    {
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
                            let r = <G::Host as Host>::tuple_borrow_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "tuple-result",
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
                            let r = <G::Host as Host>::tuple_result(&mut accessor).await;
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
                inst.func_wrap_concurrent(
                    "option-own-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Option<wasmtime::component::Resource<Bar>>,)|
                    {
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
                            let r = <G::Host as Host>::option_own_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "option-borrow-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Option<wasmtime::component::Resource<Bar>>,)|
                    {
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
                            let r = <G::Host as Host>::option_borrow_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "option-result",
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
                            let r = <G::Host as Host>::option_result(&mut accessor)
                                .await;
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
                inst.func_wrap_concurrent(
                    "result-own-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Result<wasmtime::component::Resource<Bar>, ()>,)|
                    {
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
                            let r = <G::Host as Host>::result_own_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "result-borrow-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Result<wasmtime::component::Resource<Bar>, ()>,)|
                    {
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
                            let r = <G::Host as Host>::result_borrow_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "result-result",
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
                            let r = <G::Host as Host>::result_result(&mut accessor)
                                .await;
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
                inst.func_wrap_concurrent(
                    "list-own-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (
                            arg0,
                        ): (
                            wasmtime::component::__internal::Vec<
                                wasmtime::component::Resource<Bar>,
                            >,
                        )|
                    {
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
                            let r = <G::Host as Host>::list_own_arg(&mut accessor, arg0)
                                .await;
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
                inst.func_wrap_concurrent(
                    "list-borrow-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (
                            arg0,
                        ): (
                            wasmtime::component::__internal::Vec<
                                wasmtime::component::Resource<Bar>,
                            >,
                        )|
                    {
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
                            let r = <G::Host as Host>::list_borrow_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "list-result",
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
                            let r = <G::Host as Host>::list_result(&mut accessor).await;
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
                inst.func_wrap_concurrent(
                    "record-own-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (NestedOwn,)|
                    {
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
                            let r = <G::Host as Host>::record_own_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "record-borrow-arg",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (NestedBorrow,)|
                    {
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
                            let r = <G::Host as Host>::record_borrow_arg(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                inst.func_wrap_concurrent(
                    "record-result",
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
                            let r = <G::Host as Host>::record_result(&mut accessor)
                                .await;
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
                inst.func_wrap_concurrent(
                    "func-with-handle-typedef",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (SomeHandle,)|
                    {
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
                            let r = <G::Host as Host>::func_with_handle_typedef(
                                    &mut accessor,
                                    arg0,
                                )
                                .await;
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
                U: Host + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host + Send> Host for &mut _T {
                async fn bar_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::Resource<Bar>,
                ) -> () {
                    struct Task {
                        x: wasmtime::component::Resource<Bar>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::bar_own_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn bar_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::Resource<Bar>,
                ) -> () {
                    struct Task {
                        x: wasmtime::component::Resource<Bar>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::bar_borrow_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn bar_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> wasmtime::component::Resource<Bar> {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::Resource<Bar>,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::Resource<Bar> {
                            <U as Host>::bar_result(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn tuple_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: (wasmtime::component::Resource<Bar>, u32),
                ) -> () {
                    struct Task {
                        x: (wasmtime::component::Resource<Bar>, u32),
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::tuple_own_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn tuple_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: (wasmtime::component::Resource<Bar>, u32),
                ) -> () {
                    struct Task {
                        x: (wasmtime::component::Resource<Bar>, u32),
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::tuple_borrow_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn tuple_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> (wasmtime::component::Resource<Bar>, u32) {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        (wasmtime::component::Resource<Bar>, u32),
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> (wasmtime::component::Resource<Bar>, u32) {
                            <U as Host>::tuple_result(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn option_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Option<wasmtime::component::Resource<Bar>>,
                ) -> () {
                    struct Task {
                        x: Option<wasmtime::component::Resource<Bar>>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::option_own_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn option_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Option<wasmtime::component::Resource<Bar>>,
                ) -> () {
                    struct Task {
                        x: Option<wasmtime::component::Resource<Bar>>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::option_borrow_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn option_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> Option<wasmtime::component::Resource<Bar>> {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        Option<wasmtime::component::Resource<Bar>>,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Option<wasmtime::component::Resource<Bar>> {
                            <U as Host>::option_result(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn result_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Result<wasmtime::component::Resource<Bar>, ()>,
                ) -> () {
                    struct Task {
                        x: Result<wasmtime::component::Resource<Bar>, ()>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::result_own_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn result_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Result<wasmtime::component::Resource<Bar>, ()>,
                ) -> () {
                    struct Task {
                        x: Result<wasmtime::component::Resource<Bar>, ()>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::result_borrow_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn result_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> Result<wasmtime::component::Resource<Bar>, ()> {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        Result<wasmtime::component::Resource<Bar>, ()>,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Result<wasmtime::component::Resource<Bar>, ()> {
                            <U as Host>::result_result(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn list_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                ) -> () {
                    struct Task {
                        x: wasmtime::component::__internal::Vec<
                            wasmtime::component::Resource<Bar>,
                        >,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::list_own_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn list_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                ) -> () {
                    struct Task {
                        x: wasmtime::component::__internal::Vec<
                            wasmtime::component::Resource<Bar>,
                        >,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::list_borrow_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn list_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> wasmtime::component::__internal::Vec<
                    wasmtime::component::Resource<Bar>,
                > {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::__internal::Vec<
                            wasmtime::component::Resource<Bar>,
                        >,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::__internal::Vec<
                            wasmtime::component::Resource<Bar>,
                        > {
                            <U as Host>::list_result(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn record_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: NestedOwn,
                ) -> () {
                    struct Task {
                        x: NestedOwn,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::record_own_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn record_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: NestedBorrow,
                ) -> () {
                    struct Task {
                        x: NestedBorrow,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::record_borrow_arg(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn record_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> NestedOwn {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, NestedOwn> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> NestedOwn {
                            <U as Host>::record_result(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn func_with_handle_typedef<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: SomeHandle,
                ) -> () {
                    struct Task {
                        x: SomeHandle,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::func_with_handle_typedef(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
            }
        }
        #[allow(clippy::all)]
        pub mod long_use_chain1 {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub enum A {}
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait HostA: Sized {
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<A>,
                ) -> wasmtime::Result<()>;
            }
            impl<_T: HostA> HostA for &mut _T {
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<A>,
                ) -> wasmtime::Result<()> {
                    HostA::drop(*self, rep).await
                }
            }
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send + HostA + Sized {}
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
                let mut inst = linker.instance("foo:foo/long-use-chain1")?;
                inst.resource_async(
                    "a",
                    wasmtime::component::ResourceType::host::<A>(),
                    move |mut store, rep| {
                        wasmtime::component::__internal::Box::new(async move {
                            HostA::drop(
                                    &mut host_getter(store.data_mut()),
                                    wasmtime::component::Resource::new_own(rep),
                                )
                                .await
                        })
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
            impl<_T: Host + Send> Host for &mut _T {}
        }
        #[allow(clippy::all)]
        pub mod long_use_chain2 {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub type A = super::super::super::foo::foo::long_use_chain1::A;
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {}
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
                let mut inst = linker.instance("foo:foo/long-use-chain2")?;
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
            impl<_T: Host + Send> Host for &mut _T {}
        }
        #[allow(clippy::all)]
        pub mod long_use_chain3 {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub type A = super::super::super::foo::foo::long_use_chain2::A;
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {}
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
                let mut inst = linker.instance("foo:foo/long-use-chain3")?;
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
            impl<_T: Host + Send> Host for &mut _T {}
        }
        #[allow(clippy::all)]
        pub mod long_use_chain4 {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub type A = super::super::super::foo::foo::long_use_chain3::A;
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn foo<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::Resource<A>,
                > + Send
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
                let mut inst = linker.instance("foo:foo/long-use-chain4")?;
                inst.func_wrap_concurrent(
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
                            let r = <G::Host as Host>::foo(&mut accessor).await;
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
                async fn foo<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> wasmtime::component::Resource<A> {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::Resource<A>,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::Resource<A> {
                            <U as Host>::foo(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
            }
        }
        #[allow(clippy::all)]
        pub mod transitive_interface_with_resource {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            pub enum Foo {}
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait HostFoo: Sized {
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<Foo>,
                ) -> wasmtime::Result<()>;
            }
            impl<_T: HostFoo> HostFoo for &mut _T {
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<Foo>,
                ) -> wasmtime::Result<()> {
                    HostFoo::drop(*self, rep).await
                }
            }
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send + HostFoo + Sized {}
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
                let mut inst = linker
                    .instance("foo:foo/transitive-interface-with-resource")?;
                inst.resource_async(
                    "foo",
                    wasmtime::component::ResourceType::host::<Foo>(),
                    move |mut store, rep| {
                        wasmtime::component::__internal::Box::new(async move {
                            HostFoo::drop(
                                    &mut host_getter(store.data_mut()),
                                    wasmtime::component::Resource::new_own(rep),
                                )
                                .await
                        })
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
            impl<_T: Host + Send> Host for &mut _T {}
        }
    }
}
pub mod exports {
    pub mod foo {
        pub mod foo {
            #[allow(clippy::all)]
            pub mod uses_resource_transitively {
                #[allow(unused_imports)]
                use wasmtime::component::__internal::{anyhow, Box};
                pub type Foo = super::super::super::super::foo::foo::transitive_interface_with_resource::Foo;
                pub struct Guest {
                    handle: wasmtime::component::Func,
                }
                #[derive(Clone)]
                pub struct GuestIndices {
                    handle: wasmtime::component::ComponentExportIndex,
                }
                impl GuestIndices {
                    /// Constructor for [`GuestIndices`] which takes a
                    /// [`Component`](wasmtime::component::Component) as input and can be executed
                    /// before instantiation.
                    ///
                    /// This constructor can be used to front-load string lookups to find exports
                    /// within a component.
                    pub fn new<_T>(
                        _instance_pre: &wasmtime::component::InstancePre<_T>,
                    ) -> wasmtime::Result<GuestIndices> {
                        let instance = _instance_pre
                            .component()
                            .get_export_index(None, "foo:foo/uses-resource-transitively")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/uses-resource-transitively`"
                                )
                            })?;
                        let mut lookup = move |name| {
                            _instance_pre
                                .component()
                                .get_export_index(Some(&instance), name)
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "instance export `foo:foo/uses-resource-transitively` does \
                        not have export `{name}`"
                                    )
                                })
                        };
                        let _ = &mut lookup;
                        let handle = lookup("handle")?;
                        Ok(GuestIndices { handle })
                    }
                    pub fn load(
                        &self,
                        mut store: impl wasmtime::AsContextMut,
                        instance: &wasmtime::component::Instance,
                    ) -> wasmtime::Result<Guest> {
                        let _instance = instance;
                        let _instance_pre = _instance.instance_pre(&store);
                        let _instance_type = _instance_pre.instance_type();
                        let mut store = store.as_context_mut();
                        let _ = &mut store;
                        let handle = *_instance
                            .get_typed_func::<
                                (wasmtime::component::Resource<Foo>,),
                                (),
                            >(&mut store, &self.handle)?
                            .func();
                        Ok(Guest { handle })
                    }
                }
                impl Guest {
                    pub fn call_handle<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::Resource<Foo>,
                    ) -> impl wasmtime::component::__internal::Future<
                        Output = wasmtime::Result<()>,
                    > + Send + 'static + use<S>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (wasmtime::component::Resource<Foo>,),
                                (),
                            >::new_unchecked(self.handle)
                        };
                        callee.call_concurrent(store.as_context_mut(), (arg0,))
                    }
                }
            }
        }
    }
}
