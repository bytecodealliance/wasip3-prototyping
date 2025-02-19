/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `host`.
///
/// This structure is created through [`Host_Pre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`Host_`] as well.
pub struct Host_Pre<T> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: Host_Indices,
}
impl<T> Clone for Host_Pre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T> Host_Pre<_T> {
    /// Creates a new copy of `Host_Pre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = Host_Indices::new(instance_pre.component())?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`Host_`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<Host_>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `host`.
///
/// This is an implementation detail of [`Host_Pre`] and can
/// be constructed if needed as well.
///
/// For more information see [`Host_`] as well.
#[derive(Clone)]
pub struct Host_Indices {}
/// Auto-generated bindings for an instance a component which
/// implements the world `host`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`Host_::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`Host_Pre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`Host_Pre::instantiate_async`] to
///   create a [`Host_`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`Host_::new`].
///
/// * You can also access the guts of instantiation through
///   [`Host_Indices::new_instance`] followed
///   by [`Host_Indices::load`] to crate an instance of this
///   type.
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct Host_ {}
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait Host_Imports: Send {
    fn foo<T: 'static>(
        accessor: &mut wasmtime::component::Accessor<T, Self>,
    ) -> impl ::core::future::Future<Output = ()> + Send + Sync
    where
        Self: Sized;
}
thread_local! {
    static HOST : std::cell::Cell <* mut u8 > =
    std::cell::Cell::new(std::ptr::null_mut()); static SPAWNED : std::cell::RefCell < Vec
    < wasmtime::component::__internal::Spawned >> = std::cell::RefCell::new(Vec::new());
}
fn poll<T, G: for<'a> Host_ImportsGetHost<&'a mut T>, F: std::future::Future + ?Sized>(
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
pub trait Host_ImportsGetHost<
    T,
>: Fn(T) -> <Self as Host_ImportsGetHost<T>>::Host + Send + Sync + Copy + 'static {
    type Host: Host_Imports;
}
impl<F, T, O> Host_ImportsGetHost<T> for F
where
    F: Fn(T) -> O + Send + Sync + Copy + 'static,
    O: Host_Imports,
{
    type Host = O;
}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl Host_Indices {
        /// Creates a new copy of `Host_Indices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new(
            component: &wasmtime::component::Component,
        ) -> wasmtime::Result<Self> {
            let _component = component;
            Ok(Host_Indices {})
        }
        /// Creates a new instance of [`Host_Indices`] from an
        /// instantiated component.
        ///
        /// This method of creating a [`Host_`] will perform string
        /// lookups for all exports when this method is called. This
        /// will only succeed if the provided instance matches the
        /// requirements of [`Host_`].
        pub fn new_instance(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Self> {
            let _instance = instance;
            Ok(Host_Indices {})
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`Host_`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Host_> {
            let _instance = instance;
            Ok(Host_ {})
        }
    }
    impl Host_ {
        /// Convenience wrapper around [`Host_Pre::new`] and
        /// [`Host_Pre::instantiate_async`].
        pub async fn instantiate_async<_T>(
            mut store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<Host_>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            Host_Pre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`Host_Indices::new_instance`] and
        /// [`Host_Indices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Host_> {
            let indices = Host_Indices::new_instance(&mut store, instance)?;
            indices.load(store, instance)
        }
        pub fn add_to_linker_imports_get_host<
            T,
            G: for<'a> Host_ImportsGetHost<&'a mut T, Host: Host_Imports>,
        >(
            linker: &mut wasmtime::component::Linker<T>,
            host_getter: G,
        ) -> wasmtime::Result<()>
        where
            T: Send + 'static,
        {
            let mut linker = linker.root();
            linker
                .func_wrap_concurrent(
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
                            let r = <G::Host as Host_Imports>::foo(&mut accessor).await;
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
};
