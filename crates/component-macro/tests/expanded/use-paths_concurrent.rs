/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `d`.
///
/// This structure is created through [`DPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`D`] as well.
pub struct DPre<T: 'static> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: DIndices,
}
impl<T: 'static> Clone for DPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T: 'static> DPre<_T> {
    /// Creates a new copy of `DPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = DIndices::new(&instance_pre)?;
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
        pub fn new<_T>(
            _instance_pre: &wasmtime::component::InstancePre<_T>,
        ) -> wasmtime::Result<Self> {
            let _component = _instance_pre.component();
            let _instance_type = _instance_pre.instance_type();
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
            let _ = &mut store;
            let _instance = instance;
            Ok(D {})
        }
    }
    impl D {
        /// Convenience wrapper around [`DPre::new`] and
        /// [`DPre::instantiate_async`].
        pub async fn instantiate_async<_T: 'static>(
            store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<D>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            DPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`DIndices::new`] and
        /// [`DIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<D> {
            let indices = DIndices::new(&instance.instance_pre(&store))?;
            indices.load(&mut store, instance)
        }
        pub fn add_to_linker<T, D>(
            linker: &mut wasmtime::component::Linker<T>,
            host_getter: fn(&mut T) -> D::Data<'_>,
        ) -> wasmtime::Result<()>
        where
<<<<<<< HEAD
            D: foo::foo::a::HostConcurrent + foo::foo::b::HostConcurrent
                + foo::foo::c::HostConcurrent + d::HostConcurrent + Send,
            for<'a> D::Data<
                'a,
            >: foo::foo::a::Host + foo::foo::b::Host + foo::foo::c::Host + d::Host
                + Send,
            T: 'static + Send,
||||||| 40315bd2c
            T: Send + foo::foo::a::Host<Data = T> + foo::foo::b::Host<Data = T>
                + foo::foo::c::Host<Data = T> + d::Host<Data = T> + 'static,
            U: Send + foo::foo::a::Host<Data = T> + foo::foo::b::Host<Data = T>
                + foo::foo::c::Host<Data = T> + d::Host<Data = T>,
=======
            T: 'static,
            T: Send + foo::foo::a::Host<Data = T> + foo::foo::b::Host<Data = T>
                + foo::foo::c::Host<Data = T> + d::Host<Data = T>,
            U: Send + foo::foo::a::Host<Data = T> + foo::foo::b::Host<Data = T>
                + foo::foo::c::Host<Data = T> + d::Host<Data = T>,
>>>>>>> upstream/main
        {
            foo::foo::a::add_to_linker::<T, D>(linker, host_getter)?;
            foo::foo::b::add_to_linker::<T, D>(linker, host_getter)?;
            foo::foo::c::add_to_linker::<T, D>(linker, host_getter)?;
            d::add_to_linker::<T, D>(linker, host_getter)?;
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
            pub trait HostConcurrent: wasmtime::component::HasData + Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Foo> + Send
                where
                    Self: Sized;
            }
<<<<<<< HEAD
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {}
            impl<_T: Host + Send> Host for &mut _T {}
            pub fn add_to_linker<T, D>(
||||||| 40315bd2c
            pub trait GetHost<
                T,
                D,
            >: Fn(T) -> <Self as GetHost<T, D>>::Host + Send + Sync + Copy + 'static {
                type Host: Host<Data = D> + Send;
            }
            impl<F, T, D, O> GetHost<T, D> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host<Data = D> + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, T, Host: Host<Data = T> + Send>,
            >(
=======
            pub fn add_to_linker_get_host<T, G>(
>>>>>>> upstream/main
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: fn(&mut T) -> D::Data<'_>,
            ) -> wasmtime::Result<()>
            where
<<<<<<< HEAD
                D: HostConcurrent,
                for<'a> D::Data<'a>: Host,
                T: 'static + Send,
||||||| 40315bd2c
                T: Send + 'static,
=======
                T: 'static,
                G: for<'a> wasmtime::component::GetHost<
                    &'a mut T,
                    Host: Host<Data = T> + Send,
                >,
                T: Send + 'static,
>>>>>>> upstream/main
            {
                let mut inst = linker.instance("foo:foo/a")?;
                inst.func_wrap_concurrent(
                    "a",
                    move |caller: &mut wasmtime::component::Accessor<T>, (): ()| {
                        wasmtime::component::__internal::Box::pin(async move {
                            let accessor = &mut unsafe { caller.with_data(host_getter) };
                            let r = <D as HostConcurrent>::a(accessor).await;
                            Ok((r,))
                        })
                    },
                )?;
                Ok(())
            }
<<<<<<< HEAD
||||||| 40315bd2c
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                U: Host<Data = T> + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host> Host for &mut _T {
                type Data = _T::Data;
                fn a(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Foo + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::a(store)
                }
            }
=======
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                T: 'static,
                U: Host<Data = T> + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host> Host for &mut _T {
                type Data = _T::Data;
                fn a(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Foo + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::a(store)
                }
            }
>>>>>>> upstream/main
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
            pub trait HostConcurrent: wasmtime::component::HasData + Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Foo> + Send
                where
                    Self: Sized;
            }
<<<<<<< HEAD
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {}
            impl<_T: Host + Send> Host for &mut _T {}
            pub fn add_to_linker<T, D>(
||||||| 40315bd2c
            pub trait GetHost<
                T,
                D,
            >: Fn(T) -> <Self as GetHost<T, D>>::Host + Send + Sync + Copy + 'static {
                type Host: Host<Data = D> + Send;
            }
            impl<F, T, D, O> GetHost<T, D> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host<Data = D> + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, T, Host: Host<Data = T> + Send>,
            >(
=======
            pub fn add_to_linker_get_host<T, G>(
>>>>>>> upstream/main
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: fn(&mut T) -> D::Data<'_>,
            ) -> wasmtime::Result<()>
            where
<<<<<<< HEAD
                D: HostConcurrent,
                for<'a> D::Data<'a>: Host,
                T: 'static + Send,
||||||| 40315bd2c
                T: Send + 'static,
=======
                T: 'static,
                G: for<'a> wasmtime::component::GetHost<
                    &'a mut T,
                    Host: Host<Data = T> + Send,
                >,
                T: Send + 'static,
>>>>>>> upstream/main
            {
                let mut inst = linker.instance("foo:foo/b")?;
                inst.func_wrap_concurrent(
                    "a",
                    move |caller: &mut wasmtime::component::Accessor<T>, (): ()| {
                        wasmtime::component::__internal::Box::pin(async move {
                            let accessor = &mut unsafe { caller.with_data(host_getter) };
                            let r = <D as HostConcurrent>::a(accessor).await;
                            Ok((r,))
                        })
                    },
                )?;
                Ok(())
            }
<<<<<<< HEAD
||||||| 40315bd2c
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                U: Host<Data = T> + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host> Host for &mut _T {
                type Data = _T::Data;
                fn a(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Foo + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::a(store)
                }
            }
=======
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                T: 'static,
                U: Host<Data = T> + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host> Host for &mut _T {
                type Data = _T::Data;
                fn a(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Foo + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::a(store)
                }
            }
>>>>>>> upstream/main
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
            pub trait HostConcurrent: wasmtime::component::HasData + Send {
                fn a<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Foo> + Send
                where
                    Self: Sized;
            }
<<<<<<< HEAD
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {}
            impl<_T: Host + Send> Host for &mut _T {}
            pub fn add_to_linker<T, D>(
||||||| 40315bd2c
            pub trait GetHost<
                T,
                D,
            >: Fn(T) -> <Self as GetHost<T, D>>::Host + Send + Sync + Copy + 'static {
                type Host: Host<Data = D> + Send;
            }
            impl<F, T, D, O> GetHost<T, D> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host<Data = D> + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, T, Host: Host<Data = T> + Send>,
            >(
=======
            pub fn add_to_linker_get_host<T, G>(
>>>>>>> upstream/main
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: fn(&mut T) -> D::Data<'_>,
            ) -> wasmtime::Result<()>
            where
<<<<<<< HEAD
                D: HostConcurrent,
                for<'a> D::Data<'a>: Host,
                T: 'static + Send,
||||||| 40315bd2c
                T: Send + 'static,
=======
                T: 'static,
                G: for<'a> wasmtime::component::GetHost<
                    &'a mut T,
                    Host: Host<Data = T> + Send,
                >,
                T: Send + 'static,
>>>>>>> upstream/main
            {
                let mut inst = linker.instance("foo:foo/c")?;
                inst.func_wrap_concurrent(
                    "a",
                    move |caller: &mut wasmtime::component::Accessor<T>, (): ()| {
                        wasmtime::component::__internal::Box::pin(async move {
                            let accessor = &mut unsafe { caller.with_data(host_getter) };
                            let r = <D as HostConcurrent>::a(accessor).await;
                            Ok((r,))
                        })
                    },
                )?;
                Ok(())
            }
<<<<<<< HEAD
||||||| 40315bd2c
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                U: Host<Data = T> + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host> Host for &mut _T {
                type Data = _T::Data;
                fn a(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Foo + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::a(store)
                }
            }
=======
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                T: 'static,
                U: Host<Data = T> + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host> Host for &mut _T {
                type Data = _T::Data;
                fn a(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Foo + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::a(store)
                }
            }
>>>>>>> upstream/main
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
    pub trait HostConcurrent: wasmtime::component::HasData + Send {
        fn b<T: 'static>(
            accessor: &mut wasmtime::component::Accessor<T, Self>,
        ) -> impl ::core::future::Future<Output = Foo> + Send
        where
            Self: Sized;
    }
<<<<<<< HEAD
    #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
    pub trait Host: Send {}
    impl<_T: Host + Send> Host for &mut _T {}
    pub fn add_to_linker<T, D>(
||||||| 40315bd2c
    pub trait GetHost<
        T,
        D,
    >: Fn(T) -> <Self as GetHost<T, D>>::Host + Send + Sync + Copy + 'static {
        type Host: Host<Data = D> + Send;
    }
    impl<F, T, D, O> GetHost<T, D> for F
    where
        F: Fn(T) -> O + Send + Sync + Copy + 'static,
        O: Host<Data = D> + Send,
    {
        type Host = O;
    }
    pub fn add_to_linker_get_host<
        T,
        G: for<'a> GetHost<&'a mut T, T, Host: Host<Data = T> + Send>,
    >(
=======
    pub fn add_to_linker_get_host<T, G>(
>>>>>>> upstream/main
        linker: &mut wasmtime::component::Linker<T>,
        host_getter: fn(&mut T) -> D::Data<'_>,
    ) -> wasmtime::Result<()>
    where
<<<<<<< HEAD
        D: HostConcurrent,
        for<'a> D::Data<'a>: Host,
        T: 'static + Send,
||||||| 40315bd2c
        T: Send + 'static,
=======
        T: 'static,
        G: for<'a> wasmtime::component::GetHost<&'a mut T, Host: Host<Data = T> + Send>,
        T: Send + 'static,
>>>>>>> upstream/main
    {
        let mut inst = linker.instance("d")?;
        inst.func_wrap_concurrent(
            "b",
            move |caller: &mut wasmtime::component::Accessor<T>, (): ()| {
                wasmtime::component::__internal::Box::pin(async move {
                    let accessor = &mut unsafe { caller.with_data(host_getter) };
                    let r = <D as HostConcurrent>::b(accessor).await;
                    Ok((r,))
                })
            },
        )?;
        Ok(())
    }
<<<<<<< HEAD
||||||| 40315bd2c
    pub fn add_to_linker<T, U>(
        linker: &mut wasmtime::component::Linker<T>,
        get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
    ) -> wasmtime::Result<()>
    where
        U: Host<Data = T> + Send,
        T: Send + 'static,
    {
        add_to_linker_get_host(linker, get)
    }
    impl<_T: Host> Host for &mut _T {
        type Data = _T::Data;
        fn b(
            store: wasmtime::StoreContextMut<'_, Self::Data>,
        ) -> impl ::core::future::Future<
            Output = impl FnOnce(
                wasmtime::StoreContextMut<'_, Self::Data>,
            ) -> Foo + Send + Sync + 'static,
        > + Send + Sync + 'static
        where
            Self: Sized,
        {
            <_T as Host>::b(store)
        }
    }
=======
    pub fn add_to_linker<T, U>(
        linker: &mut wasmtime::component::Linker<T>,
        get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
    ) -> wasmtime::Result<()>
    where
        T: 'static,
        U: Host<Data = T> + Send,
        T: Send + 'static,
    {
        add_to_linker_get_host(linker, get)
    }
    impl<_T: Host> Host for &mut _T {
        type Data = _T::Data;
        fn b(
            store: wasmtime::StoreContextMut<'_, Self::Data>,
        ) -> impl ::core::future::Future<
            Output = impl FnOnce(
                wasmtime::StoreContextMut<'_, Self::Data>,
            ) -> Foo + Send + Sync + 'static,
        > + Send + Sync + 'static
        where
            Self: Sized,
        {
            <_T as Host>::b(store)
        }
    }
>>>>>>> upstream/main
}
