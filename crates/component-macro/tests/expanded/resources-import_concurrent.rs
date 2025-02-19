pub enum WorldResource {}
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait HostWorldResource: Sized {
    fn new<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<
        Output = wasmtime::component::Resource<WorldResource>,
    > + Send + Sync
    where
        Self: Sized;
    fn foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
        self_: wasmtime::component::Resource<WorldResource>,
    ) -> impl ::core::future::Future<Output = ()> + Send + Sync
    where
        Self: Sized;
    fn static_foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<Output = ()> + Send + Sync
    where
        Self: Sized;
    async fn drop(
        &mut self,
        rep: wasmtime::component::Resource<WorldResource>,
    ) -> wasmtime::Result<()>;
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
    interface1: exports::foo::foo::uses_resource_transitively::Guest,
    some_world_func2: wasmtime::component::Func,
}
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait TheWorldImports: Send + HostWorldResource {
    fn some_world_func<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<
        Output = wasmtime::component::Resource<WorldResource>,
    > + Send + Sync
    where
        Self: Sized;
}
thread_local! {
    static HOST : std::cell::Cell <* mut u8 > =
    std::cell::Cell::new(std::ptr::null_mut()); static SPAWNED : std::cell::RefCell < Vec
    < wasmtime::component::__internal::Spawned >> = std::cell::RefCell::new(Vec::new());
}
fn poll<
    T,
    G: for<'a> TheWorldImportsGetHost<&'a mut T>,
    F: std::future::Future + ?Sized,
>(
    getter: G,
    store: wasmtime::VMStoreRawPtr,
    cx: &mut std::task::Context,
    future: std::pin::Pin<&mut F>,
) -> std::task::Poll<F::Output> {
    use wasmtime::component::__internal::SpawnedInner;
    use std::mem;
    use std::ops::DerefMut;
    use std::task::Poll;
    struct ResetHost(*mut u8);
    impl Drop for ResetHost {
        fn drop(&mut self) {
            HOST.with(|v| v.set(self.0))
        }
    }
    struct ClearSpawned;
    impl Drop for ClearSpawned {
        fn drop(&mut self) {
            SPAWNED.with(|v| v.borrow_mut().clear())
        }
    }
    let mut store_cx = unsafe {
        wasmtime::StoreContextMut::new(&mut *store.0.as_ptr().cast())
    };
    let _clear = ClearSpawned;
    let result = {
        let host = &mut getter(store_cx.data_mut());
        let old = HOST.with(|v| v.replace((host as *mut G::Host).cast()));
        let _reset = ResetHost(old);
        future.poll(cx)
    };
    for spawned in SPAWNED
        .with(|v| { mem::take(DerefMut::deref_mut(&mut v.borrow_mut())) })
    {
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
                        let result = poll(getter, store, cx, future.as_mut());
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
            let interface1 = exports::foo::foo::uses_resource_transitively::GuestIndices::new(
                _component,
            )?;
            let some_world_func2 = _component
                .export_index(None, "some-world-func2")
                .ok_or_else(|| {
                    anyhow::anyhow!("no function export `some-world-func2` found")
                })?
                .1;
            Ok(TheWorldIndices {
                interface1,
                some_world_func2,
            })
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
            let interface1 = exports::foo::foo::uses_resource_transitively::GuestIndices::new_instance(
                &mut store,
                _instance,
            )?;
            let some_world_func2 = _instance
                .get_export(&mut store, None, "some-world-func2")
                .ok_or_else(|| {
                    anyhow::anyhow!("no function export `some-world-func2` found")
                })?;
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
                    "[constructor]world-resource",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as HostWorldResource>::new(&mut accessor)
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
            linker
                .func_wrap_concurrent(
                    "[method]world-resource.foo",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (wasmtime::component::Resource<WorldResource>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
            linker
                .func_wrap_concurrent(
                    "[static]world-resource.static-foo",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as HostWorldResource>::static_foo(
                                    &mut accessor,
                                )
                                .await;
                            Ok(r)
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
            linker
                .func_wrap_concurrent(
                    "some-world-func",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as TheWorldImports>::some_world_func(
                                    &mut accessor,
                                )
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
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
        pub async fn call_some_world_func2<S: wasmtime::AsContextMut>(
            &self,
            mut store: S,
        ) -> wasmtime::Result<
            wasmtime::component::Promise<wasmtime::component::Resource<WorldResource>>,
        >
        where
            <S as wasmtime::AsContext>::Data: Send,
        {
            let callee = unsafe {
                wasmtime::component::TypedFunc::<
                    (),
                    (wasmtime::component::Resource<WorldResource>,),
                >::new_unchecked(self.some_world_func2)
            };
            let promise = callee.call_concurrent(store.as_context_mut(), ()).await?;
            Ok(promise.map(|(v,)| v))
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
                > + Send + Sync
                where
                    Self: Sized;
                fn static_a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = u32> + Send + Sync
                where
                    Self: Sized;
                fn method_a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    self_: wasmtime::component::Resource<Bar>,
                ) -> impl ::core::future::Future<Output = u32> + Send + Sync
                where
                    Self: Sized;
                async fn drop(
                    &mut self,
                    rep: wasmtime::component::Resource<Bar>,
                ) -> wasmtime::Result<()>;
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
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn bar_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::Resource<Bar>,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn bar_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::Resource<Bar>,
                > + Send + Sync
                where
                    Self: Sized;
                fn tuple_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: (wasmtime::component::Resource<Bar>, u32),
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn tuple_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: (wasmtime::component::Resource<Bar>, u32),
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn tuple_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = (wasmtime::component::Resource<Bar>, u32),
                > + Send + Sync
                where
                    Self: Sized;
                fn option_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Option<wasmtime::component::Resource<Bar>>,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn option_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Option<wasmtime::component::Resource<Bar>>,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn option_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = Option<wasmtime::component::Resource<Bar>>,
                > + Send + Sync
                where
                    Self: Sized;
                fn result_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Result<wasmtime::component::Resource<Bar>, ()>,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn result_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: Result<wasmtime::component::Resource<Bar>, ()>,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn result_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = Result<wasmtime::component::Resource<Bar>, ()>,
                > + Send + Sync
                where
                    Self: Sized;
                fn list_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn list_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn list_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = wasmtime::component::__internal::Vec<
                        wasmtime::component::Resource<Bar>,
                    >,
                > + Send + Sync
                where
                    Self: Sized;
                fn record_own_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: NestedOwn,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn record_borrow_arg<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: NestedBorrow,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
                fn record_result<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = NestedOwn> + Send + Sync
                where
                    Self: Sized;
                fn func_with_handle_typedef<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                    x: SomeHandle,
                ) -> impl ::core::future::Future<Output = ()> + Send + Sync
                where
                    Self: Sized;
            }
            thread_local! {
                static HOST : std::cell::Cell <* mut u8 > =
                std::cell::Cell::new(std::ptr::null_mut()); static SPAWNED :
                std::cell::RefCell < Vec < wasmtime::component::__internal::Spawned >> =
                std::cell::RefCell::new(Vec::new());
            }
            fn poll<T, G: for<'a> GetHost<&'a mut T>, F: std::future::Future + ?Sized>(
                getter: G,
                store: wasmtime::VMStoreRawPtr,
                cx: &mut std::task::Context,
                future: std::pin::Pin<&mut F>,
            ) -> std::task::Poll<F::Output> {
                use wasmtime::component::__internal::SpawnedInner;
                use std::mem;
                use std::ops::DerefMut;
                use std::task::Poll;
                struct ResetHost(*mut u8);
                impl Drop for ResetHost {
                    fn drop(&mut self) {
                        HOST.with(|v| v.set(self.0))
                    }
                }
                struct ClearSpawned;
                impl Drop for ClearSpawned {
                    fn drop(&mut self) {
                        SPAWNED.with(|v| v.borrow_mut().clear())
                    }
                }
                let mut store_cx = unsafe {
                    wasmtime::StoreContextMut::new(&mut *store.0.as_ptr().cast())
                };
                let _clear = ClearSpawned;
                let result = {
                    let host = &mut getter(store_cx.data_mut());
                    let old = HOST.with(|v| v.replace((host as *mut G::Host).cast()));
                    let _reset = ResetHost(old);
                    future.poll(cx)
                };
                for spawned in SPAWNED
                    .with(|v| { mem::take(DerefMut::deref_mut(&mut v.borrow_mut())) })
                {
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
                                    let result = poll(getter, store, cx, future.as_mut());
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
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as HostBar>::new(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "[static]bar.static-a",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as HostBar>::static_a(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "[method]bar.method-a",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (wasmtime::component::Resource<Bar>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as HostBar>::method_a(&mut accessor, arg0)
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "bar-own-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (wasmtime::component::Resource<Bar>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::bar_own_arg(&mut accessor, arg0)
                                .await;
                            Ok(r)
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "bar-borrow-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (wasmtime::component::Resource<Bar>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "bar-result",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::bar_result(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "tuple-own-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): ((wasmtime::component::Resource<Bar>, u32),)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::tuple_own_arg(&mut accessor, arg0)
                                .await;
                            Ok(r)
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "tuple-borrow-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): ((wasmtime::component::Resource<Bar>, u32),)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "tuple-result",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::tuple_result(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "option-own-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (Option<wasmtime::component::Resource<Bar>>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "option-borrow-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (Option<wasmtime::component::Resource<Bar>>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "option-result",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::option_result(&mut accessor)
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "result-own-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (Result<wasmtime::component::Resource<Bar>, ()>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "result-borrow-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (Result<wasmtime::component::Resource<Bar>, ()>,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "result-result",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::result_result(&mut accessor)
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "list-own-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (
                            arg0,
                        ): (
                            wasmtime::component::__internal::Vec<
                                wasmtime::component::Resource<Bar>,
                            >,
                        )|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::list_own_arg(&mut accessor, arg0)
                                .await;
                            Ok(r)
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "list-borrow-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (
                            arg0,
                        ): (
                            wasmtime::component::__internal::Vec<
                                wasmtime::component::Resource<Bar>,
                            >,
                        )|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "list-result",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::list_result(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "record-own-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (NestedOwn,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "record-borrow-arg",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (NestedBorrow,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "record-result",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::record_result(&mut accessor)
                                .await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
                                host_getter,
                                store,
                                cx,
                                future.as_mut(),
                            )),
                        )
                    },
                )?;
                inst.func_wrap_concurrent(
                    "func-with-handle-typedef",
                    move |
                        mut caller: wasmtime::StoreContextMut<'_, T>,
                        (arg0,): (SomeHandle,)|
                    {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
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
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
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
                > + Send + Sync
                where
                    Self: Sized;
            }
            thread_local! {
                static HOST : std::cell::Cell <* mut u8 > =
                std::cell::Cell::new(std::ptr::null_mut()); static SPAWNED :
                std::cell::RefCell < Vec < wasmtime::component::__internal::Spawned >> =
                std::cell::RefCell::new(Vec::new());
            }
            fn poll<T, G: for<'a> GetHost<&'a mut T>, F: std::future::Future + ?Sized>(
                getter: G,
                store: wasmtime::VMStoreRawPtr,
                cx: &mut std::task::Context,
                future: std::pin::Pin<&mut F>,
            ) -> std::task::Poll<F::Output> {
                use wasmtime::component::__internal::SpawnedInner;
                use std::mem;
                use std::ops::DerefMut;
                use std::task::Poll;
                struct ResetHost(*mut u8);
                impl Drop for ResetHost {
                    fn drop(&mut self) {
                        HOST.with(|v| v.set(self.0))
                    }
                }
                struct ClearSpawned;
                impl Drop for ClearSpawned {
                    fn drop(&mut self) {
                        SPAWNED.with(|v| v.borrow_mut().clear())
                    }
                }
                let mut store_cx = unsafe {
                    wasmtime::StoreContextMut::new(&mut *store.0.as_ptr().cast())
                };
                let _clear = ClearSpawned;
                let result = {
                    let host = &mut getter(store_cx.data_mut());
                    let old = HOST.with(|v| v.replace((host as *mut G::Host).cast()));
                    let _reset = ResetHost(old);
                    future.poll(cx)
                };
                for spawned in SPAWNED
                    .with(|v| { mem::take(DerefMut::deref_mut(&mut v.borrow_mut())) })
                {
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
                                    let result = poll(getter, store, cx, future.as_mut());
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
                let mut inst = linker.instance("foo:foo/long-use-chain4")?;
                inst.func_wrap_concurrent(
                    "foo",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        _ = host_getter;
                        let mut accessor = unsafe {
                            wasmtime::component::Accessor::new(
                                caller.inner(),
                                || HOST.with(|v| v.get()).cast(),
                                |future| SPAWNED.with(|v| v.borrow_mut().push(future)),
                            )
                        };
                        let mut future = wasmtime::component::__internal::Box::pin(async move {
                            let r = <G::Host as Host>::foo(&mut accessor).await;
                            Ok((r,))
                        });
                        let store = wasmtime::VMStoreRawPtr(caller.traitobj());
                        wasmtime::component::__internal::Box::pin(
                            wasmtime::component::__internal::poll_fn(move |cx| poll(
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
                    pub fn new(
                        component: &wasmtime::component::Component,
                    ) -> wasmtime::Result<GuestIndices> {
                        let (_, instance) = component
                            .export_index(None, "foo:foo/uses-resource-transitively")
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/uses-resource-transitively`"
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
                            .get_export(
                                &mut store,
                                None,
                                "foo:foo/uses-resource-transitively",
                            )
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "no exported instance named `foo:foo/uses-resource-transitively`"
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
                        let mut store = store.as_context_mut();
                        let _ = &mut store;
                        let _instance = instance;
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
                    pub async fn call_handle<S: wasmtime::AsContextMut>(
                        &self,
                        mut store: S,
                        arg0: wasmtime::component::Resource<Foo>,
                    ) -> wasmtime::Result<wasmtime::component::Promise<()>>
                    where
                        <S as wasmtime::AsContext>::Data: Send,
                    {
                        let callee = unsafe {
                            wasmtime::component::TypedFunc::<
                                (wasmtime::component::Resource<Foo>,),
                                (),
                            >::new_unchecked(self.handle)
                        };
                        let promise = callee
                            .call_concurrent(store.as_context_mut(), (arg0,))
                            .await?;
                        Ok(promise)
                    }
                }
            }
        }
    }
}
