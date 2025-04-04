/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `the-flags`.
///
/// This structure is created through [`TheFlagsPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`TheFlags`] as well.
pub struct TheFlagsPre<T> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: TheFlagsIndices,
}
impl<T> Clone for TheFlagsPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T> TheFlagsPre<_T> {
    /// Creates a new copy of `TheFlagsPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = TheFlagsIndices::new(instance_pre.component())?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`TheFlags`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<TheFlags>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `the-flags`.
///
/// This is an implementation detail of [`TheFlagsPre`] and can
/// be constructed if needed as well.
///
/// For more information see [`TheFlags`] as well.
#[derive(Clone)]
pub struct TheFlagsIndices {
    interface0: exports::foo::foo::flegs::GuestIndices,
}
/// Auto-generated bindings for an instance a component which
/// implements the world `the-flags`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`TheFlags::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`TheFlagsPre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`TheFlagsPre::instantiate_async`] to
///   create a [`TheFlags`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`TheFlags::new`].
///
/// * You can also access the guts of instantiation through
///   [`TheFlagsIndices::new_instance`] followed
///   by [`TheFlagsIndices::load`] to crate an instance of this
///   type.
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct TheFlags {
    interface0: exports::foo::foo::flegs::Guest,
}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl TheFlagsIndices {
        /// Creates a new copy of `TheFlagsIndices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new(
            component: &wasmtime::component::Component,
        ) -> wasmtime::Result<Self> {
            let _component = component;
            let interface0 = exports::foo::foo::flegs::GuestIndices::new(_component)?;
            Ok(TheFlagsIndices { interface0 })
        }
        /// Creates a new instance of [`TheFlagsIndices`] from an
        /// instantiated component.
        ///
        /// This method of creating a [`TheFlags`] will perform string
        /// lookups for all exports when this method is called. This
        /// will only succeed if the provided instance matches the
        /// requirements of [`TheFlags`].
        pub fn new_instance(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Self> {
            let _instance = instance;
            let interface0 = exports::foo::foo::flegs::GuestIndices::new_instance(
                &mut store,
                _instance,
            )?;
            Ok(TheFlagsIndices { interface0 })
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`TheFlags`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<TheFlags> {
            let _instance = instance;
            let interface0 = self.interface0.load(&mut store, &_instance)?;
            Ok(TheFlags { interface0 })
        }
    }
    impl TheFlags {
        /// Convenience wrapper around [`TheFlagsPre::new`] and
        /// [`TheFlagsPre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            mut store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<TheFlags>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            TheFlagsPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`TheFlagsIndices::new_instance`] and
        /// [`TheFlagsIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<TheFlags> {
            let indices = TheFlagsIndices::new_instance(&mut store, instance)?;
            indices.load(store, instance)
        }
        pub fn add_to_linker<T, U>(
            linker: &mut wasmtime::component::Linker<T>,
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
            U: foo::foo::flegs::Host + Send,
        {
            foo::foo::flegs::add_to_linker(linker, get)?;
            Ok(())
        }
        pub fn foo_foo_flegs(&self) -> &exports::foo::foo::flegs::Guest {
            &self.interface0
        }
    }
};
pub mod foo {
    pub mod foo {
        #[allow(clippy::all)]
        pub mod flegs {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            wasmtime::component::flags!(Flag1 { #[component(name = "b0")] const B0; });
            const _: () = {
                assert!(1 == < Flag1 as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Flag1 as wasmtime::component::ComponentType >::ALIGN32);
            };
            wasmtime::component::flags!(
                Flag2 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                const B1; }
            );
            const _: () = {
                assert!(1 == < Flag2 as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Flag2 as wasmtime::component::ComponentType >::ALIGN32);
            };
            wasmtime::component::flags!(
                Flag4 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                const B1; #[component(name = "b2")] const B2; #[component(name = "b3")]
                const B3; }
            );
            const _: () = {
                assert!(1 == < Flag4 as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Flag4 as wasmtime::component::ComponentType >::ALIGN32);
            };
            wasmtime::component::flags!(
                Flag8 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                const B1; #[component(name = "b2")] const B2; #[component(name = "b3")]
                const B3; #[component(name = "b4")] const B4; #[component(name = "b5")]
                const B5; #[component(name = "b6")] const B6; #[component(name = "b7")]
                const B7; }
            );
            const _: () = {
                assert!(1 == < Flag8 as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Flag8 as wasmtime::component::ComponentType >::ALIGN32);
            };
            wasmtime::component::flags!(
                Flag16 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                const B1; #[component(name = "b2")] const B2; #[component(name = "b3")]
                const B3; #[component(name = "b4")] const B4; #[component(name = "b5")]
                const B5; #[component(name = "b6")] const B6; #[component(name = "b7")]
                const B7; #[component(name = "b8")] const B8; #[component(name = "b9")]
                const B9; #[component(name = "b10")] const B10; #[component(name =
                "b11")] const B11; #[component(name = "b12")] const B12; #[component(name
                = "b13")] const B13; #[component(name = "b14")] const B14;
                #[component(name = "b15")] const B15; }
            );
            const _: () = {
                assert!(2 == < Flag16 as wasmtime::component::ComponentType >::SIZE32);
                assert!(2 == < Flag16 as wasmtime::component::ComponentType >::ALIGN32);
            };
            wasmtime::component::flags!(
                Flag32 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                const B1; #[component(name = "b2")] const B2; #[component(name = "b3")]
                const B3; #[component(name = "b4")] const B4; #[component(name = "b5")]
                const B5; #[component(name = "b6")] const B6; #[component(name = "b7")]
                const B7; #[component(name = "b8")] const B8; #[component(name = "b9")]
                const B9; #[component(name = "b10")] const B10; #[component(name =
                "b11")] const B11; #[component(name = "b12")] const B12; #[component(name
                = "b13")] const B13; #[component(name = "b14")] const B14;
                #[component(name = "b15")] const B15; #[component(name = "b16")] const
                B16; #[component(name = "b17")] const B17; #[component(name = "b18")]
                const B18; #[component(name = "b19")] const B19; #[component(name =
                "b20")] const B20; #[component(name = "b21")] const B21; #[component(name
                = "b22")] const B22; #[component(name = "b23")] const B23;
                #[component(name = "b24")] const B24; #[component(name = "b25")] const
                B25; #[component(name = "b26")] const B26; #[component(name = "b27")]
                const B27; #[component(name = "b28")] const B28; #[component(name =
                "b29")] const B29; #[component(name = "b30")] const B30; #[component(name
                = "b31")] const B31; }
            );
            const _: () = {
                assert!(4 == < Flag32 as wasmtime::component::ComponentType >::SIZE32);
                assert!(4 == < Flag32 as wasmtime::component::ComponentType >::ALIGN32);
            };
            wasmtime::component::flags!(
                Flag64 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                const B1; #[component(name = "b2")] const B2; #[component(name = "b3")]
                const B3; #[component(name = "b4")] const B4; #[component(name = "b5")]
                const B5; #[component(name = "b6")] const B6; #[component(name = "b7")]
                const B7; #[component(name = "b8")] const B8; #[component(name = "b9")]
                const B9; #[component(name = "b10")] const B10; #[component(name =
                "b11")] const B11; #[component(name = "b12")] const B12; #[component(name
                = "b13")] const B13; #[component(name = "b14")] const B14;
                #[component(name = "b15")] const B15; #[component(name = "b16")] const
                B16; #[component(name = "b17")] const B17; #[component(name = "b18")]
                const B18; #[component(name = "b19")] const B19; #[component(name =
                "b20")] const B20; #[component(name = "b21")] const B21; #[component(name
                = "b22")] const B22; #[component(name = "b23")] const B23;
                #[component(name = "b24")] const B24; #[component(name = "b25")] const
                B25; #[component(name = "b26")] const B26; #[component(name = "b27")]
                const B27; #[component(name = "b28")] const B28; #[component(name =
                "b29")] const B29; #[component(name = "b30")] const B30; #[component(name
                = "b31")] const B31; #[component(name = "b32")] const B32;
                #[component(name = "b33")] const B33; #[component(name = "b34")] const
                B34; #[component(name = "b35")] const B35; #[component(name = "b36")]
                const B36; #[component(name = "b37")] const B37; #[component(name =
                "b38")] const B38; #[component(name = "b39")] const B39; #[component(name
                = "b40")] const B40; #[component(name = "b41")] const B41;
                #[component(name = "b42")] const B42; #[component(name = "b43")] const
                B43; #[component(name = "b44")] const B44; #[component(name = "b45")]
                const B45; #[component(name = "b46")] const B46; #[component(name =
                "b47")] const B47; #[component(name = "b48")] const B48; #[component(name
                = "b49")] const B49; #[component(name = "b50")] const B50;
                #[component(name = "b51")] const B51; #[component(name = "b52")] const
                B52; #[component(name = "b53")] const B53; #[component(name = "b54")]
                const B54; #[component(name = "b55")] const B55; #[component(name =
                "b56")] const B56; #[component(name = "b57")] const B57; #[component(name
                = "b58")] const B58; #[component(name = "b59")] const B59;
                #[component(name = "b60")] const B60; #[component(name = "b61")] const
                B61; #[component(name = "b62")] const B62; #[component(name = "b63")]
                const B63; }
            );
            const _: () = {
                assert!(8 == < Flag64 as wasmtime::component::ComponentType >::SIZE32);
                assert!(4 == < Flag64 as wasmtime::component::ComponentType >::ALIGN32);
            };
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {
                fn roundtrip_flag1<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag1,
                ) -> impl ::core::future::Future<Output = Flag1> + Send
                where
                    Self: Sized;
                fn roundtrip_flag2<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag2,
                ) -> impl ::core::future::Future<Output = Flag2> + Send
                where
                    Self: Sized;
                fn roundtrip_flag4<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag4,
                ) -> impl ::core::future::Future<Output = Flag4> + Send
                where
                    Self: Sized;
                fn roundtrip_flag8<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag8,
                ) -> impl ::core::future::Future<Output = Flag8> + Send
                where
                    Self: Sized;
                fn roundtrip_flag16<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag16,
                ) -> impl ::core::future::Future<Output = Flag16> + Send
                where
                    Self: Sized;
                fn roundtrip_flag32<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag32,
                ) -> impl ::core::future::Future<Output = Flag32> + Send
                where
                    Self: Sized;
                fn roundtrip_flag64<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag64,
                ) -> impl ::core::future::Future<Output = Flag64> + Send
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
                let mut inst = linker.instance("foo:foo/flegs")?;
                inst.func_wrap_concurrent(
                    "roundtrip-flag1",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag1,)|
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
                            let r = <G::Host as Host>::roundtrip_flag1(
                                    &mut accessor,
                                    arg0,
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
                inst.func_wrap_concurrent(
                    "roundtrip-flag2",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag2,)|
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
                            let r = <G::Host as Host>::roundtrip_flag2(
                                    &mut accessor,
                                    arg0,
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
                inst.func_wrap_concurrent(
                    "roundtrip-flag4",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag4,)|
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
                            let r = <G::Host as Host>::roundtrip_flag4(
                                    &mut accessor,
                                    arg0,
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
                inst.func_wrap_concurrent(
                    "roundtrip-flag8",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag8,)|
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
                            let r = <G::Host as Host>::roundtrip_flag8(
                                    &mut accessor,
                                    arg0,
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
                inst.func_wrap_concurrent(
                    "roundtrip-flag16",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag16,)|
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
                            let r = <G::Host as Host>::roundtrip_flag16(
                                    &mut accessor,
                                    arg0,
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
                inst.func_wrap_concurrent(
                    "roundtrip-flag32",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag32,)|
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
                            let r = <G::Host as Host>::roundtrip_flag32(
                                    &mut accessor,
                                    arg0,
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
                inst.func_wrap_concurrent(
                    "roundtrip-flag64",
                    move |
                        caller: &mut wasmtime::component::Accessor<T, T>,
                        (arg0,): (Flag64,)|
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
                            let r = <G::Host as Host>::roundtrip_flag64(
                                    &mut accessor,
                                    arg0,
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
                async fn roundtrip_flag1<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag1,
                ) -> Flag1 {
                    struct Task {
                        x: Flag1,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag1> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag1 {
                            <U as Host>::roundtrip_flag1(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn roundtrip_flag2<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag2,
                ) -> Flag2 {
                    struct Task {
                        x: Flag2,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag2> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag2 {
                            <U as Host>::roundtrip_flag2(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn roundtrip_flag4<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag4,
                ) -> Flag4 {
                    struct Task {
                        x: Flag4,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag4> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag4 {
                            <U as Host>::roundtrip_flag4(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn roundtrip_flag8<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag8,
                ) -> Flag8 {
                    struct Task {
                        x: Flag8,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag8> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag8 {
                            <U as Host>::roundtrip_flag8(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn roundtrip_flag16<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag16,
                ) -> Flag16 {
                    struct Task {
                        x: Flag16,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag16> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag16 {
                            <U as Host>::roundtrip_flag16(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn roundtrip_flag32<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag32,
                ) -> Flag32 {
                    struct Task {
                        x: Flag32,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag32> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag32 {
                            <U as Host>::roundtrip_flag32(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
                async fn roundtrip_flag64<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Flag64,
                ) -> Flag64 {
                    struct Task {
                        x: Flag64,
                    }
                    impl<
                        T: 'static,
                        U: Host,
                    > wasmtime::component::AccessorTask<T, U, Flag64> for Task {
                        async fn run(
                            self,
                            accessor: &mut wasmtime::component::Accessor<T, U>,
                        ) -> Flag64 {
                            <U as Host>::roundtrip_flag64(accessor, self.x).await
                        }
                    }
                    accessor.forward(|v| *v, Task { x }).await
                }
            }
        }
    }
}
pub mod exports {
    pub mod foo {
        pub mod foo {
            #[allow(clippy::all)]
            pub mod flegs {
                #[allow(unused_imports)]
                use wasmtime::component::__internal::{anyhow, Box};
                wasmtime::component::flags!(
                    Flag1 { #[component(name = "b0")] const B0; }
                );
                const _: () = {
                    assert!(
                        1 == < Flag1 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        1 == < Flag1 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                wasmtime::component::flags!(
                    Flag2 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                    const B1; }
                );
                const _: () = {
                    assert!(
                        1 == < Flag2 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        1 == < Flag2 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                wasmtime::component::flags!(
                    Flag4 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                    const B1; #[component(name = "b2")] const B2; #[component(name =
                    "b3")] const B3; }
                );
                const _: () = {
                    assert!(
                        1 == < Flag4 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        1 == < Flag4 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                wasmtime::component::flags!(
                    Flag8 { #[component(name = "b0")] const B0; #[component(name = "b1")]
                    const B1; #[component(name = "b2")] const B2; #[component(name =
                    "b3")] const B3; #[component(name = "b4")] const B4; #[component(name
                    = "b5")] const B5; #[component(name = "b6")] const B6;
                    #[component(name = "b7")] const B7; }
                );
                const _: () = {
                    assert!(
                        1 == < Flag8 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        1 == < Flag8 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                wasmtime::component::flags!(
                    Flag16 { #[component(name = "b0")] const B0; #[component(name =
                    "b1")] const B1; #[component(name = "b2")] const B2; #[component(name
                    = "b3")] const B3; #[component(name = "b4")] const B4;
                    #[component(name = "b5")] const B5; #[component(name = "b6")] const
                    B6; #[component(name = "b7")] const B7; #[component(name = "b8")]
                    const B8; #[component(name = "b9")] const B9; #[component(name =
                    "b10")] const B10; #[component(name = "b11")] const B11;
                    #[component(name = "b12")] const B12; #[component(name = "b13")]
                    const B13; #[component(name = "b14")] const B14; #[component(name =
                    "b15")] const B15; }
                );
                const _: () = {
                    assert!(
                        2 == < Flag16 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        2 == < Flag16 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                wasmtime::component::flags!(
                    Flag32 { #[component(name = "b0")] const B0; #[component(name =
                    "b1")] const B1; #[component(name = "b2")] const B2; #[component(name
                    = "b3")] const B3; #[component(name = "b4")] const B4;
                    #[component(name = "b5")] const B5; #[component(name = "b6")] const
                    B6; #[component(name = "b7")] const B7; #[component(name = "b8")]
                    const B8; #[component(name = "b9")] const B9; #[component(name =
                    "b10")] const B10; #[component(name = "b11")] const B11;
                    #[component(name = "b12")] const B12; #[component(name = "b13")]
                    const B13; #[component(name = "b14")] const B14; #[component(name =
                    "b15")] const B15; #[component(name = "b16")] const B16;
                    #[component(name = "b17")] const B17; #[component(name = "b18")]
                    const B18; #[component(name = "b19")] const B19; #[component(name =
                    "b20")] const B20; #[component(name = "b21")] const B21;
                    #[component(name = "b22")] const B22; #[component(name = "b23")]
                    const B23; #[component(name = "b24")] const B24; #[component(name =
                    "b25")] const B25; #[component(name = "b26")] const B26;
                    #[component(name = "b27")] const B27; #[component(name = "b28")]
                    const B28; #[component(name = "b29")] const B29; #[component(name =
                    "b30")] const B30; #[component(name = "b31")] const B31; }
                );
                const _: () = {
                    assert!(
                        4 == < Flag32 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        4 == < Flag32 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                wasmtime::component::flags!(
                    Flag64 { #[component(name = "b0")] const B0; #[component(name =
                    "b1")] const B1; #[component(name = "b2")] const B2; #[component(name
                    = "b3")] const B3; #[component(name = "b4")] const B4;
                    #[component(name = "b5")] const B5; #[component(name = "b6")] const
                    B6; #[component(name = "b7")] const B7; #[component(name = "b8")]
                    const B8; #[component(name = "b9")] const B9; #[component(name =
                    "b10")] const B10; #[component(name = "b11")] const B11;
                    #[component(name = "b12")] const B12; #[component(name = "b13")]
                    const B13; #[component(name = "b14")] const B14; #[component(name =
                    "b15")] const B15; #[component(name = "b16")] const B16;
                    #[component(name = "b17")] const B17; #[component(name = "b18")]
                    const B18; #[component(name = "b19")] const B19; #[component(name =
                    "b20")] const B20; #[component(name = "b21")] const B21;
                    #[component(name = "b22")] const B22; #[component(name = "b23")]
                    const B23; #[component(name = "b24")] const B24; #[component(name =
                    "b25")] const B25; #[component(name = "b26")] const B26;
                    #[component(name = "b27")] const B27; #[component(name = "b28")]
                    const B28; #[component(name = "b29")] const B29; #[component(name =
                    "b30")] const B30; #[component(name = "b31")] const B31;
                    #[component(name = "b32")] const B32; #[component(name = "b33")]
                    const B33; #[component(name = "b34")] const B34; #[component(name =
                    "b35")] const B35; #[component(name = "b36")] const B36;
                    #[component(name = "b37")] const B37; #[component(name = "b38")]
                    const B38; #[component(name = "b39")] const B39; #[component(name =
                    "b40")] const B40; #[component(name = "b41")] const B41;
                    #[component(name = "b42")] const B42; #[component(name = "b43")]
                    const B43; #[component(name = "b44")] const B44; #[component(name =
                    "b45")] const B45; #[component(name = "b46")] const B46;
                    #[component(name = "b47")] const B47; #[component(name = "b48")]
                    const B48; #[component(name = "b49")] const B49; #[component(name =
                    "b50")] const B50; #[component(name = "b51")] const B51;
                    #[component(name = "b52")] const B52; #[component(name = "b53")]
                    const B53; #[component(name = "b54")] const B54; #[component(name =
                    "b55")] const B55; #[component(name = "b56")] const B56;
                    #[component(name = "b57")] const B57; #[component(name = "b58")]
                    const B58; #[component(name = "b59")] const B59; #[component(name =
                    "b60")] const B60; #[component(name = "b61")] const B61;
                    #[component(name = "b62")] const B62; #[component(name = "b63")]
                    const B63; }
                );
                const _: () = {
                    assert!(
                        8 == < Flag64 as wasmtime::component::ComponentType >::SIZE32
                    );
                    assert!(
                        4 == < Flag64 as wasmtime::component::ComponentType >::ALIGN32
                    );
                };
                pub struct Guest {
                    roundtrip_flag1: wasmtime::component::Func,
                    roundtrip_flag2: wasmtime::component::Func,
                    roundtrip_flag4: wasmtime::component::Func,
                    roundtrip_flag8: wasmtime::component::Func,
                    roundtrip_flag16: wasmtime::component::Func,
                    roundtrip_flag32: wasmtime::component::Func,
                    roundtrip_flag64: wasmtime::component::Func,
                }
                #[derive(Clone)]
                pub struct GuestIndices {
                    roundtrip_flag1: wasmtime::component::ComponentExportIndex,
                    roundtrip_flag2: wasmtime::component::ComponentExportIndex,
                    roundtrip_flag4: wasmtime::component::ComponentExportIndex,
                    roundtrip_flag8: wasmtime::component::ComponentExportIndex,
                    roundtrip_flag16: wasmtime::component::ComponentExportIndex,
                    roundtrip_flag32: wasmtime::component::ComponentExportIndex,
                    roundtrip_flag64: wasmtime::component::ComponentExportIndex,
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
                            .export_index(None, "foo:foo/flegs")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/flegs`"
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
                            .get_export(&mut store, None, "foo:foo/flegs")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/flegs`"
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
                                        "instance export `foo:foo/flegs` does \
                not have export `{name}`"
                                    )
                                })
                        };
                        let _ = &mut lookup;
                        let roundtrip_flag1 = lookup("roundtrip-flag1")?;
                        let roundtrip_flag2 = lookup("roundtrip-flag2")?;
                        let roundtrip_flag4 = lookup("roundtrip-flag4")?;
                        let roundtrip_flag8 = lookup("roundtrip-flag8")?;
                        let roundtrip_flag16 = lookup("roundtrip-flag16")?;
                        let roundtrip_flag32 = lookup("roundtrip-flag32")?;
                        let roundtrip_flag64 = lookup("roundtrip-flag64")?;
                        Ok(GuestIndices {
                            roundtrip_flag1,
                            roundtrip_flag2,
                            roundtrip_flag4,
                            roundtrip_flag8,
                            roundtrip_flag16,
                            roundtrip_flag32,
                            roundtrip_flag64,
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
                        let roundtrip_flag1 = *_instance
                            .get_typed_func::<
                                (Flag1,),
                                (Flag1,),
                            >(&mut store, &self.roundtrip_flag1)?
                            .func();
                        let roundtrip_flag2 = *_instance
                            .get_typed_func::<
                                (Flag2,),
                                (Flag2,),
                            >(&mut store, &self.roundtrip_flag2)?
                            .func();
                        let roundtrip_flag4 = *_instance
                            .get_typed_func::<
                                (Flag4,),
                                (Flag4,),
                            >(&mut store, &self.roundtrip_flag4)?
                            .func();
                        let roundtrip_flag8 = *_instance
                            .get_typed_func::<
                                (Flag8,),
                                (Flag8,),
                            >(&mut store, &self.roundtrip_flag8)?
                            .func();
                        let roundtrip_flag16 = *_instance
                            .get_typed_func::<
                                (Flag16,),
                                (Flag16,),
                            >(&mut store, &self.roundtrip_flag16)?
                            .func();
                        let roundtrip_flag32 = *_instance
                            .get_typed_func::<
                                (Flag32,),
                                (Flag32,),
                            >(&mut store, &self.roundtrip_flag32)?
                            .func();
                        let roundtrip_flag64 = *_instance
                            .get_typed_func::<
                                (Flag64,),
                                (Flag64,),
                            >(&mut store, &self.roundtrip_flag64)?
                            .func();
                        Ok(Guest {
                            roundtrip_flag1,
                            roundtrip_flag2,
                            roundtrip_flag4,
                            roundtrip_flag8,
                            roundtrip_flag16,
                            roundtrip_flag32,
                            roundtrip_flag64,
                        })
                    }
                }
                impl Guest {
                    pub async fn call_roundtrip_flag1<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag1,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag1>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag1,),
                                (Flag1,),
                            >::new_unchecked(self.roundtrip_flag1)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_roundtrip_flag2<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag2,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag2>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag2,),
                                (Flag2,),
                            >::new_unchecked(self.roundtrip_flag2)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_roundtrip_flag4<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag4,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag4>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag4,),
                                (Flag4,),
                            >::new_unchecked(self.roundtrip_flag4)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_roundtrip_flag8<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag8,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag8>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag8,),
                                (Flag8,),
                            >::new_unchecked(self.roundtrip_flag8)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_roundtrip_flag16<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag16,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag16>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag16,),
                                (Flag16,),
                            >::new_unchecked(self.roundtrip_flag16)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_roundtrip_flag32<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag32,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag32>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag32,),
                                (Flag32,),
                            >::new_unchecked(self.roundtrip_flag32)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise.map(|(v,)| v))
                    }
                    pub async fn call_roundtrip_flag64<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: Flag64,
                    ) -> wasmtime::Result<wasmtime::component::Promise<Flag64>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (Flag64,),
                                (Flag64,),
                            >::new_unchecked(self.roundtrip_flag64)
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
