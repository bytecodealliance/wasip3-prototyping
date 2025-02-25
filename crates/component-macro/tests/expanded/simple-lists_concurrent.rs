/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `my-world`.
///
/// This structure is created through [`MyWorldPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`MyWorld`] as well.
pub struct MyWorldPre<T> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: MyWorldIndices,
}
impl<T> Clone for MyWorldPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T> MyWorldPre<_T> {
    /// Creates a new copy of `MyWorldPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = MyWorldIndices::new(instance_pre.component())?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`MyWorld`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<MyWorld>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `my-world`.
///
/// This is an implementation detail of [`MyWorldPre`] and can
/// be constructed if needed as well.
///
/// For more information see [`MyWorld`] as well.
#[derive(Clone)]
pub struct MyWorldIndices {
    interface0: exports::foo::foo::simple_lists::GuestIndices,
}
/// Auto-generated bindings for an instance a component which
/// implements the world `my-world`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`MyWorld::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`MyWorldPre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`MyWorldPre::instantiate_async`] to
///   create a [`MyWorld`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`MyWorld::new`].
///
/// * You can also access the guts of instantiation through
///   [`MyWorldIndices::new_instance`] followed
///   by [`MyWorldIndices::load`] to crate an instance of this
///   type.
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct MyWorld {
    interface0: exports::foo::foo::simple_lists::Guest,
}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl MyWorldIndices {
        /// Creates a new copy of `MyWorldIndices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new(
            component: &wasmtime::component::Component,
        ) -> wasmtime::Result<Self> {
            let _component = component;
            let interface0 = exports::foo::foo::simple_lists::GuestIndices::new(
                _component,
            )?;
            Ok(MyWorldIndices { interface0 })
        }
        /// Creates a new instance of [`MyWorldIndices`] from an
        /// instantiated component.
        ///
        /// This method of creating a [`MyWorld`] will perform string
        /// lookups for all exports when this method is called. This
        /// will only succeed if the provided instance matches the
        /// requirements of [`MyWorld`].
        pub fn new_instance(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Self> {
            let _instance = instance;
            let interface0 = exports::foo::foo::simple_lists::GuestIndices::new_instance(
                &mut store,
                _instance,
            )?;
            Ok(MyWorldIndices { interface0 })
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`MyWorld`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<MyWorld> {
            let _instance = instance;
            let interface0 = self.interface0.load(&mut store, &_instance)?;
            Ok(MyWorld { interface0 })
        }
    }
    impl MyWorld {
        /// Convenience wrapper around [`MyWorldPre::new`] and
        /// [`MyWorldPre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            mut store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<MyWorld>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            MyWorldPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`MyWorldIndices::new_instance`] and
        /// [`MyWorldIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<MyWorld> {
            let indices = MyWorldIndices::new_instance(&mut store, instance)?;
            indices.load(store, instance)
        }
        pub fn add_to_linker<T, U>(
            linker: &mut wasmtime::component::Linker<T>,
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
            U: foo::foo::simple_lists::Host + Send,
        {
            foo::foo::simple_lists::add_to_linker(linker, get)?;
            Ok(())
        }
        pub fn foo_foo_simple_lists(&self) -> &exports::foo::foo::simple_lists::Guest {
            &self.interface0
        }
    }
};
pub mod foo {
    pub mod foo {
        #[allow(clippy::all)]
        pub mod simple_lists {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn simple_list1<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    l: wasmtime::component::__internal::Vec<u32>,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn simple_list2<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::__internal::Vec<u32>,
                > + Send + Sync
                where
                    Self: Sized;
                fn simple_list3<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    a: wasmtime::component::__internal::Vec<u32>,
                    b: wasmtime::component::__internal::Vec<u32>,
                ) -> impl ::core::future::Future<
                    Output = (
                        wasmtime::component::__internal::Vec<u32>,
                        wasmtime::component::__internal::Vec<u32>,
                    ),
                > + Send + Sync
                where
                    Self: Sized;
                fn simple_list4<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    l: wasmtime::component::__internal::Vec<
                        wasmtime::component::__internal::Vec<u32>,
                    >,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::__internal::Vec<
                        wasmtime::component::__internal::Vec<u32>,
                    >,
                > + Send + Sync
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
                    store_cx
                        .spawn(
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
                let mut inst = linker.instance("foo:foo/simple-lists")?;
                inst.func_wrap_concurrent(
                    "simple-list1",
                    move |
                        caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (wasmtime::component::__internal::Vec<u32>,)|
                    {
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::<
                                T,
                                _,
                            >::new(get_host_and_store, spawn_task)
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::simple_list1(&mut accessor, arg0)
                                .await;
                            Ok(r)
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll_with_state(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "simple-list2",
                    move |caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::<
                                T,
                                _,
                            >::new(get_host_and_store, spawn_task)
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::simple_list2(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll_with_state(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "simple-list3",
                    move |
                        caller: wasmtime::StoreContextMut<'_, T>,
                        (
                            arg0,
                            arg1,
                        ): (
                            wasmtime::component::__internal::Vec<u32>,
                            wasmtime::component::__internal::Vec<u32>,
                        )|
                    {
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::<
                                T,
                                _,
                            >::new(get_host_and_store, spawn_task)
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::simple_list3(
                                    &mut accessor,
                                    arg0,
                                    arg1,
                                )
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll_with_state(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "simple-list4",
                    move |
                        caller: wasmtime::StoreContextMut<'_, T>,
                        (
                            arg0,
                        ): (
                            wasmtime::component::__internal::Vec<
                                wasmtime::component::__internal::Vec<u32>,
                            >,
                        )|
                    {
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::<
                                T,
                                _,
                            >::new(get_host_and_store, spawn_task)
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::simple_list4(&mut accessor, arg0)
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll_with_state(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
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
                async fn simple_list1<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    l: wasmtime::component::__internal::Vec<u32>,
                ) -> () {
                    struct Task {
                        l: wasmtime::component::__internal::Vec<u32>,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::simple_list1(accessor, self.l).await
                        }
                    }
                    accessor.forward(|v| *v, Task { l }).await
                }
                async fn simple_list2<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> wasmtime::component::__internal::Vec<u32> {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::__internal::Vec<u32>,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::__internal::Vec<u32> {
                            <U as Host>::simple_list2(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn simple_list3<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    a: wasmtime::component::__internal::Vec<u32>,
                    b: wasmtime::component::__internal::Vec<u32>,
                ) -> (
                    wasmtime::component::__internal::Vec<u32>,
                    wasmtime::component::__internal::Vec<u32>,
                ) {
                    struct Task {
                        a: wasmtime::component::__internal::Vec<u32>,
                        b: wasmtime::component::__internal::Vec<u32>,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        (
                            wasmtime::component::__internal::Vec<u32>,
                            wasmtime::component::__internal::Vec<u32>,
                        ),
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> (
                            wasmtime::component::__internal::Vec<u32>,
                            wasmtime::component::__internal::Vec<u32>,
                        ) {
                            <U as Host>::simple_list3(accessor, self.a, self.b).await
                        }
                    }
                    accessor.forward(|v| *v, Task { a, b }).await
                }
                async fn simple_list4<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    l: wasmtime::component::__internal::Vec<
                        wasmtime::component::__internal::Vec<u32>,
                    >,
                ) -> wasmtime::component::__internal::Vec<
                    wasmtime::component::__internal::Vec<u32>,
                > {
                    struct Task {
                        l: wasmtime::component::__internal::Vec<
                            wasmtime::component::__internal::Vec<u32>,
                        >,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::__internal::Vec<
                            wasmtime::component::__internal::Vec<u32>,
                        >,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::__internal::Vec<
                            wasmtime::component::__internal::Vec<u32>,
                        > {
                            <U as Host>::simple_list4(accessor, self.l).await
                        }
                    }
                    accessor.forward(|v| *v, Task { l }).await
                }
            }
        }
    }
}
pub mod exports {
    pub mod foo {
        pub mod foo {
            #[allow(clippy::all)]
            pub mod simple_lists {
                #[allow(unused_imports)]
                use wasmtime::component::__internal::{anyhow, Box};
                pub struct Guest {
                    simple_list1: wasmtime::component::Func,
                    simple_list2: wasmtime::component::Func,
                    simple_list3: wasmtime::component::Func,
                    simple_list4: wasmtime::component::Func,
                }
                #[derive(Clone)]
                pub struct GuestIndices {
                    simple_list1: wasmtime::component::ComponentExportIndex,
                    simple_list2: wasmtime::component::ComponentExportIndex,
                    simple_list3: wasmtime::component::ComponentExportIndex,
                    simple_list4: wasmtime::component::ComponentExportIndex,
                }
                impl GuestIndices {
                    /// Constructor for [`GuestIndices`] which takes a
                    /// [`Component`](wasmtime::component::Component) as input and can be executed
                    /// before instantiation.
                    ///
                    /// This constructor can be used to front-load string lookups to find exports
                    /// within a component.
                    pub fn new(
                        component: &wasmtime::component::Component,
                    ) -> wasmtime::Result<GuestIndices> {
                        let (_, instance) = component
                            .export_index(None, "foo:foo/simple-lists")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/simple-lists`"
                                )
                            })?;
                        Self::_new(|name| {
                            component.export_index(Some(&instance), name).map(|p| p.1)
                        })
                    }
                    /// This constructor is similar to [`GuestIndices::new`] except that it
                    /// performs string lookups after instantiation time.
                    pub fn new_instance(
                        mut store: impl wasmtime::AsContextMut,
                        instance: &wasmtime::component::Instance,
                    ) -> wasmtime::Result<GuestIndices> {
                        let instance_export = instance
                            .get_export(&mut store, None, "foo:foo/simple-lists")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/simple-lists`"
                                )
                            })?;
                        Self::_new(|name| {
                            instance.get_export(&mut store, Some(&instance_export), name)
                        })
                    }
                    fn _new(
                        mut lookup: impl FnMut(
                            &str,
                        ) -> Option<wasmtime::component::ComponentExportIndex>,
                    ) -> wasmtime::Result<GuestIndices> {
                        let mut lookup = move |name| {
                            lookup(name)
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "instance export `foo:foo/simple-lists` does \
                not have export `{name}`"
                                    )
                                })
                        };
                        let _ = &mut lookup;
                        let simple_list1 = lookup("simple-list1")?;
                        let simple_list2 = lookup("simple-list2")?;
                        let simple_list3 = lookup("simple-list3")?;
                        let simple_list4 = lookup("simple-list4")?;
                        Ok(GuestIndices {
                            simple_list1,
                            simple_list2,
                            simple_list3,
                            simple_list4,
                        })
                    }
                    pub fn load(
                        &self,
                        mut store: impl wasmtime::AsContextMut,
                        instance: &wasmtime::component::Instance,
                    ) -> wasmtime::Result<Guest> {
                        let mut store = store.as_context_mut();
                        let _ = &mut store;
                        let _instance = instance;
                        let simple_list1 = *_instance
                            .get_typed_func::<
                                (&[u32],),
                                (),
                            >(&mut store, &self.simple_list1)?
                            .func();
                        let simple_list2 = *_instance
                            .get_typed_func::<
                                (),
                                (wasmtime::component::__internal::Vec<u32>,),
                            >(&mut store, &self.simple_list2)?
                            .func();
                        let simple_list3 = *_instance
                            .get_typed_func::<
                                (&[u32], &[u32]),
                                (
                                    (
                                        wasmtime::component::__internal::Vec<u32>,
                                        wasmtime::component::__internal::Vec<u32>,
                                    ),
                                ),
                            >(&mut store, &self.simple_list3)?
                            .func();
                        let simple_list4 = *_instance
                            .get_typed_func::<
                                (&[wasmtime::component::__internal::Vec<u32>],),
                                (
                                    wasmtime::component::__internal::Vec<
                                        wasmtime::component::__internal::Vec<u32>,
                                    >,
                                ),
                            >(&mut store, &self.simple_list4)?
                            .func();
                        Ok(Guest {
                            simple_list1,
                            simple_list2,
                            simple_list3,
                            simple_list4,
                        })
                    }
                }
                impl Guest {
                    pub async fn call_simple_list1<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::__internal::Vec<u32>,
                    ) -> wasmtime::Result<wasmtime::component::Promise<()>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (wasmtime::component::__internal::Vec<u32>,),
                                (),
                            >::new_unchecked(self.simple_list1)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise)
                    }
                    pub async fn call_simple_list2<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                    ) -> wasmtime::Result<
                        wasmtime::component::Promise<
                            wasmtime::component::__internal::Vec<u32>,
                        >,
                    >
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (),
                                (wasmtime::component::__internal::Vec<u32>,),
                            >::new_unchecked(self.simple_list2)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), ())
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_simple_list3<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::__internal::Vec<u32>,
                        arg1: wasmtime::component::__internal::Vec<u32>,
                    ) -> wasmtime::Result<
                        wasmtime::component::Promise<
                            (
                                wasmtime::component::__internal::Vec<u32>,
                                wasmtime::component::__internal::Vec<u32>,
                            ),
                        >,
                    >
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (
                                    wasmtime::component::__internal::Vec<u32>,
                                    wasmtime::component::__internal::Vec<u32>,
                                ),
                                (
                                    (
                                        wasmtime::component::__internal::Vec<u32>,
                                        wasmtime::component::__internal::Vec<u32>,
                                    ),
                                ),
                            >::new_unchecked(self.simple_list3)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0, arg1))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_simple_list4<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::__internal::Vec<
                            wasmtime::component::__internal::Vec<u32>,
                        >,
                    ) -> wasmtime::Result<
                        wasmtime::component::Promise<
                            wasmtime::component::__internal::Vec<
                                wasmtime::component::__internal::Vec<u32>,
                            >,
                        >,
                    >
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (
                                    wasmtime::component::__internal::Vec<
                                        wasmtime::component::__internal::Vec<u32>,
                                    >,
                                ),
                                (
                                    wasmtime::component::__internal::Vec<
                                        wasmtime::component::__internal::Vec<u32>,
                                    >,
                                ),
                            >::new_unchecked(self.simple_list4)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                }
            }
        }
    }
}
