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
        let indices = TheWorldIndices::new(instance_pre.component())?;
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
    interface0: exports::foo::foo::strings::GuestIndices,
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
/// * You can also access the guts of instantiation through
///   [`TheWorldIndices::new_instance`] followed
///   by [`TheWorldIndices::load`] to crate an instance of this
///   type.
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct TheWorld {
    interface0: exports::foo::foo::strings::Guest,
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
        pub fn new(
            component: &wasmtime::component::Component,
        ) -> wasmtime::Result<Self> {
            let _component = component;
            let interface0 = exports::foo::foo::strings::GuestIndices::new(_component)?;
            Ok(TheWorldIndices { interface0 })
        }
        /// Creates a new instance of [`TheWorldIndices`] from an
        /// instantiated component.
        ///
        /// This method of creating a [`TheWorld`] will perform string
        /// lookups for all exports when this method is called. This
        /// will only succeed if the provided instance matches the
        /// requirements of [`TheWorld`].
        pub fn new_instance(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Self> {
            let _instance = instance;
            let interface0 = exports::foo::foo::strings::GuestIndices::new_instance(
                &mut store,
                _instance,
            )?;
            Ok(TheWorldIndices { interface0 })
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
            let _instance = instance;
            let interface0 = self.interface0.load(&mut store, &_instance)?;
            Ok(TheWorld { interface0 })
        }
    }
    impl TheWorld {
        /// Convenience wrapper around [`TheWorldPre::new`] and
        /// [`TheWorldPre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            mut store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<TheWorld>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            TheWorldPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`TheWorldIndices::new_instance`] and
        /// [`TheWorldIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<TheWorld> {
            let indices = TheWorldIndices::new_instance(&mut store, instance)?;
            indices.load(store, instance)
        }
        pub fn add_to_linker<T, U>(
            linker: &mut wasmtime::component::Linker<T>,
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
            U: foo::foo::strings::Host + Send,
        {
            foo::foo::strings::add_to_linker(linker, get)?;
            Ok(())
        }
        pub fn foo_foo_strings(&self) -> &exports::foo::foo::strings::Guest {
            &self.interface0
        }
    }
};
pub mod foo {
    pub mod foo {
        #[allow(clippy::all)]
        pub mod strings {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::String,
                ) -> impl ::core::future::Future<Output = ()> + Send
                where
                    Self: Sized;
                fn b<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::__internal::String,
                > + Send
                where
                    Self: Sized;
                fn c<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    a: wasmtime::component::__internal::String,
                    b: wasmtime::component::__internal::String,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::__internal::String,
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
                let mut inst = linker.instance("foo:foo/strings")?;
                inst.func_wrap_concurrent(
                    "a",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (wasmtime::component::__internal::String,)|
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
                            let r = <G::Host as Host>::a(&mut accessor, arg0).await;
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
                    "b",
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
                inst.func_wrap_concurrent(
                    "c",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (
                            arg0,
                            arg1,
                        ): (
                            wasmtime::component::__internal::String,
                            wasmtime::component::__internal::String,
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
                            let r = <G::Host as Host>::c(&mut accessor, arg0, arg1)
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
                    x: wasmtime::component::__internal::String,
                ) -> () {
                    struct Task {
                        x: wasmtime::component::__internal::String,
                    }
                    impl<T: 'static, U: Host> wasmtime::component::AccessorTask<T, U, ()>
                    for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> () {
                            <U as Host>::a(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn b<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> wasmtime::component::__internal::String {
                    struct Task {}
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::__internal::String,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::__internal::String {
                            <U as Host>::b(accessor).await
                        }
                    }
                    accessor.forward(|v| *v, Task {}).await
                }
                async fn c<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    a: wasmtime::component::__internal::String,
                    b: wasmtime::component::__internal::String,
                ) -> wasmtime::component::__internal::String {
                    struct Task {
                        a: wasmtime::component::__internal::String,
                        b: wasmtime::component::__internal::String,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<
                        T,
                        U,
                        wasmtime::component::__internal::String,
                    > for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> wasmtime::component::__internal::String {
                            <U as Host>::c(accessor, self.a, self.b).await
                        }
                    }
                    accessor.forward(|v| *v, Task { a, b }).await
                }
            }
        }
    }
}
pub mod exports {
    pub mod foo {
        pub mod foo {
            #[allow(clippy::all)]
            pub mod strings {
                #[allow(unused_imports)]
                use wasmtime::component::__internal::{anyhow, Box};
                pub struct Guest {
                    a: wasmtime::component::Func,
                    b: wasmtime::component::Func,
                    c: wasmtime::component::Func,
                }
                #[derive(Clone)]
                pub struct GuestIndices {
                    a: wasmtime::component::ComponentExportIndex,
                    b: wasmtime::component::ComponentExportIndex,
                    c: wasmtime::component::ComponentExportIndex,
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
                        let instance = component
                            .get_export_index(None, "foo:foo/strings")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/strings`"
                                )
                            })?;
                        Self::_new(|name| {
                            component.get_export_index(Some(&instance), name)
                        })
                    }
                    /// This constructor is similar to [`GuestIndices::new`] except that it
                    /// performs string lookups after instantiation time.
                    pub fn new_instance(
                        mut store: impl wasmtime::AsContextMut,
                        instance: &wasmtime::component::Instance,
                    ) -> wasmtime::Result<GuestIndices> {
                        let instance_export = instance
                            .get_export_index(&mut store, None, "foo:foo/strings")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/strings`"
                                )
                            })?;
                        Self::_new(|name| {
                            instance
                                .get_export_index(&mut store, Some(&instance_export), name)
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
                                        "instance export `foo:foo/strings` does \
                not have export `{name}`"
                                    )
                                })
                        };
                        let _ = &mut lookup;
                        let a = lookup("a")?;
                        let b = lookup("b")?;
                        let c = lookup("c")?;
                        Ok(GuestIndices { a, b, c })
                    }
                    pub fn load(
                        &self,
                        mut store: impl wasmtime::AsContextMut,
                        instance: &wasmtime::component::Instance,
                    ) -> wasmtime::Result<Guest> {
                        let mut store = store.as_context_mut();
                        let _ = &mut store;
                        let _instance = instance;
                        let a = *_instance
                            .get_typed_func::<(&str,), ()>(&mut store, &self.a)?
                            .func();
                        let b = *_instance
                            .get_typed_func::<
                                (),
                                (wasmtime::component::__internal::String,),
                            >(&mut store, &self.b)?
                            .func();
                        let c = *_instance
                            .get_typed_func::<
                                (&str, &str),
                                (wasmtime::component::__internal::String,),
                            >(&mut store, &self.c)?
                            .func();
                        Ok(Guest { a, b, c })
                    }
                }
                impl Guest {
                    pub fn call_a<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::__internal::String,
                    ) -> impl wasmtime::component::__internal::Future<
                        Output = wasmtime::Result<()>,
                    > + Send + 'static + use<S>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (wasmtime::component::__internal::String,),
                                (),
                            >::new_unchecked(self.a)
                        };
                        callee.call_concurrent(store.as_context_mut(), (arg0,))
                    }
                    pub fn call_b<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                    ) -> impl wasmtime::component::__internal::Future<
                        Output = wasmtime::Result<
                            wasmtime::component::__internal::String,
                        >,
                    > + Send + 'static + use<S>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (),
                                (wasmtime::component::__internal::String,),
                            >::new_unchecked(self.b)
                        };
                        wasmtime::component::__internal::FutureExt::map(
                            callee.call_concurrent(store.as_context_mut(), ()),
                            |v| v.map(|(v,)| v),
                        )
                    }
                    pub fn call_c<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::__internal::String,
                        arg1: wasmtime::component::__internal::String,
                    ) -> impl wasmtime::component::__internal::Future<
                        Output = wasmtime::Result<
                            wasmtime::component::__internal::String,
                        >,
                    > + Send + 'static + use<S>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (
                                    wasmtime::component::__internal::String,
                                    wasmtime::component::__internal::String,
                                ),
                                (wasmtime::component::__internal::String,),
                            >::new_unchecked(self.c)
                        };
                        wasmtime::component::__internal::FutureExt::map(
                            callee.call_concurrent(store.as_context_mut(), (arg0, arg1)),
                            |v| v.map(|(v,)| v),
                        )
                    }
                }
            }
        }
    }
}
