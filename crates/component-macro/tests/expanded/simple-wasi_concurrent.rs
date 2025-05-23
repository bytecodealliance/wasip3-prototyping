/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `wasi`.
///
/// This structure is created through [`WasiPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`Wasi`] as well.
pub struct WasiPre<T: 'static> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: WasiIndices,
}
impl<T: 'static> Clone for WasiPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T: 'static> WasiPre<_T> {
    /// Creates a new copy of `WasiPre` bindings which can then
    /// be used to instantiate into a particular store.
    ///
    /// This method may fail if the component behind `instance_pre`
    /// does not have the required exports.
    pub fn new(
        instance_pre: wasmtime::component::InstancePre<_T>,
    ) -> wasmtime::Result<Self> {
        let indices = WasiIndices::new(&instance_pre)?;
        Ok(Self { instance_pre, indices })
    }
    pub fn engine(&self) -> &wasmtime::Engine {
        self.instance_pre.engine()
    }
    pub fn instance_pre(&self) -> &wasmtime::component::InstancePre<_T> {
        &self.instance_pre
    }
    /// Instantiates a new instance of [`Wasi`] within the
    /// `store` provided.
    ///
    /// This function will use `self` as the pre-instantiated
    /// instance to perform instantiation. Afterwards the preloaded
    /// indices in `self` are used to lookup all exports on the
    /// resulting instance.
    pub async fn instantiate_async(
        &self,
        mut store: impl wasmtime::AsContextMut<Data = _T>,
    ) -> wasmtime::Result<Wasi>
    where
        _T: Send,
    {
        let mut store = store.as_context_mut();
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        self.indices.load(&mut store, &instance)
    }
}
/// Auto-generated bindings for index of the exports of
/// `wasi`.
///
/// This is an implementation detail of [`WasiPre`] and can
/// be constructed if needed as well.
///
/// For more information see [`Wasi`] as well.
#[derive(Clone)]
pub struct WasiIndices {}
/// Auto-generated bindings for an instance a component which
/// implements the world `wasi`.
///
/// This structure can be created through a number of means
/// depending on your requirements and what you have on hand:
///
/// * The most convenient way is to use
///   [`Wasi::instantiate_async`] which only needs a
///   [`Store`], [`Component`], and [`Linker`].
///
/// * Alternatively you can create a [`WasiPre`] ahead of
///   time with a [`Component`] to front-load string lookups
///   of exports once instead of per-instantiation. This
///   method then uses [`WasiPre::instantiate_async`] to
///   create a [`Wasi`].
///
/// * If you've instantiated the instance yourself already
///   then you can use [`Wasi::new`].
///
/// These methods are all equivalent to one another and move
/// around the tradeoff of what work is performed when.
///
/// [`Store`]: wasmtime::Store
/// [`Component`]: wasmtime::component::Component
/// [`Linker`]: wasmtime::component::Linker
pub struct Wasi {}
const _: () = {
    #[allow(unused_imports)]
    use wasmtime::component::__internal::anyhow;
    impl WasiIndices {
        /// Creates a new copy of `WasiIndices` bindings which can then
        /// be used to instantiate into a particular store.
        ///
        /// This method may fail if the component does not have the
        /// required exports.
        pub fn new<_T>(
            _instance_pre: &wasmtime::component::InstancePre<_T>,
        ) -> wasmtime::Result<Self> {
            let _component = _instance_pre.component();
            let _instance_type = _instance_pre.instance_type();
            Ok(WasiIndices {})
        }
        /// Uses the indices stored in `self` to load an instance
        /// of [`Wasi`] from the instance provided.
        ///
        /// Note that at this time this method will additionally
        /// perform type-checks of all exports.
        pub fn load(
            &self,
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Wasi> {
            let _ = &mut store;
            let _instance = instance;
            Ok(Wasi {})
        }
    }
    impl Wasi {
        /// Convenience wrapper around [`WasiPre::new`] and
        /// [`WasiPre::instantiate_async`].
        pub async fn instantiate_async<_T: 'static>(
            store: impl wasmtime::AsContextMut<Data = _T>,
            component: &wasmtime::component::Component,
            linker: &wasmtime::component::Linker<_T>,
        ) -> wasmtime::Result<Wasi>
        where
            _T: Send,
        {
            let pre = linker.instantiate_pre(component)?;
            WasiPre::new(pre)?.instantiate_async(store).await
        }
        /// Convenience wrapper around [`WasiIndices::new`] and
        /// [`WasiIndices::load`].
        pub fn new(
            mut store: impl wasmtime::AsContextMut,
            instance: &wasmtime::component::Instance,
        ) -> wasmtime::Result<Wasi> {
            let indices = WasiIndices::new(&instance.instance_pre(&store))?;
            indices.load(&mut store, instance)
        }
        pub fn add_to_linker<T, D>(
            linker: &mut wasmtime::component::Linker<T>,
            host_getter: fn(&mut T) -> D::Data<'_>,
        ) -> wasmtime::Result<()>
        where
<<<<<<< HEAD
            D: foo::foo::wasi_filesystem::HostConcurrent + Send,
            for<'a> D::Data<
                'a,
            >: foo::foo::wasi_filesystem::Host + foo::foo::wall_clock::Host + Send,
            T: 'static + Send,
||||||| 40315bd2c
            T: Send + foo::foo::wasi_filesystem::Host<Data = T>
                + foo::foo::wall_clock::Host + 'static,
            U: Send + foo::foo::wasi_filesystem::Host<Data = T>
                + foo::foo::wall_clock::Host,
=======
            T: 'static,
            T: Send + foo::foo::wasi_filesystem::Host<Data = T>
                + foo::foo::wall_clock::Host,
            U: Send + foo::foo::wasi_filesystem::Host<Data = T>
                + foo::foo::wall_clock::Host,
>>>>>>> upstream/main
        {
            foo::foo::wasi_filesystem::add_to_linker::<T, D>(linker, host_getter)?;
            foo::foo::wall_clock::add_to_linker::<T, D>(linker, host_getter)?;
            Ok(())
        }
    }
};
pub mod foo {
    pub mod foo {
        #[allow(clippy::all)]
        pub mod wasi_filesystem {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            #[derive(wasmtime::component::ComponentType)]
            #[derive(wasmtime::component::Lift)]
            #[derive(wasmtime::component::Lower)]
            #[component(record)]
            #[derive(Clone, Copy)]
            pub struct DescriptorStat {}
            impl core::fmt::Debug for DescriptorStat {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.debug_struct("DescriptorStat").finish()
                }
            }
            const _: () = {
                assert!(
                    0 == < DescriptorStat as wasmtime::component::ComponentType >::SIZE32
                );
                assert!(
                    1 == < DescriptorStat as wasmtime::component::ComponentType
                    >::ALIGN32
                );
            };
            #[derive(wasmtime::component::ComponentType)]
            #[derive(wasmtime::component::Lift)]
            #[derive(wasmtime::component::Lower)]
            #[component(enum)]
            #[derive(Clone, Copy, Eq, PartialEq)]
            #[repr(u8)]
            pub enum Errno {
                #[component(name = "e")]
                E,
            }
            impl Errno {
                pub fn name(&self) -> &'static str {
                    match self {
                        Errno::E => "e",
                    }
                }
                pub fn message(&self) -> &'static str {
                    match self {
                        Errno::E => "",
                    }
                }
            }
            impl core::fmt::Debug for Errno {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.debug_struct("Errno")
                        .field("code", &(*self as i32))
                        .field("name", &self.name())
                        .field("message", &self.message())
                        .finish()
                }
            }
            impl core::fmt::Display for Errno {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    write!(f, "{} (error {})", self.name(), * self as i32)
                }
            }
            impl core::error::Error for Errno {}
            const _: () = {
                assert!(1 == < Errno as wasmtime::component::ComponentType >::SIZE32);
                assert!(1 == < Errno as wasmtime::component::ComponentType >::ALIGN32);
            };
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait HostConcurrent: wasmtime::component::HasData + Send {
                fn create_directory_at<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<Output = Result<(), Errno>> + Send
                where
                    Self: Sized;
                fn stat<T: 'static>(
                    accessor: &mut wasmtime::component::Accessor<T, Self>,
                ) -> impl ::core::future::Future<
                    Output = Result<DescriptorStat, Errno>,
                > + Send
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
                let mut inst = linker.instance("foo:foo/wasi-filesystem")?;
                inst.func_wrap_concurrent(
                    "create-directory-at",
                    move |caller: &mut wasmtime::component::Accessor<T>, (): ()| {
                        wasmtime::component::__internal::Box::pin(async move {
                            let accessor = &mut unsafe { caller.with_data(host_getter) };
                            let r = <D as HostConcurrent>::create_directory_at(accessor)
                                .await;
                            Ok((r,))
                        })
                    },
                )?;
                inst.func_wrap_concurrent(
                    "stat",
                    move |caller: &mut wasmtime::component::Accessor<T>, (): ()| {
                        wasmtime::component::__internal::Box::pin(async move {
                            let accessor = &mut unsafe { caller.with_data(host_getter) };
                            let r = <D as HostConcurrent>::stat(accessor).await;
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
                fn create_directory_at(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Result<(), Errno> + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::create_directory_at(store)
                }
                fn stat(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Result<DescriptorStat, Errno> + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::stat(store)
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
                fn create_directory_at(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Result<(), Errno> + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::create_directory_at(store)
                }
                fn stat(
                    store: wasmtime::StoreContextMut<'_, Self::Data>,
                ) -> impl ::core::future::Future<
                    Output = impl FnOnce(
                        wasmtime::StoreContextMut<'_, Self::Data>,
                    ) -> Result<DescriptorStat, Errno> + Send + Sync + 'static,
                > + Send + Sync + 'static
                where
                    Self: Sized,
                {
                    <_T as Host>::stat(store)
                }
            }
>>>>>>> upstream/main
        }
        #[allow(clippy::all)]
        pub mod wall_clock {
            #[allow(unused_imports)]
            use wasmtime::component::__internal::{anyhow, Box};
            #[derive(wasmtime::component::ComponentType)]
            #[derive(wasmtime::component::Lift)]
            #[derive(wasmtime::component::Lower)]
            #[component(record)]
            #[derive(Clone, Copy)]
            pub struct WallClock {}
            impl core::fmt::Debug for WallClock {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    f.debug_struct("WallClock").finish()
                }
            }
            const _: () = {
                assert!(
                    0 == < WallClock as wasmtime::component::ComponentType >::SIZE32
                );
                assert!(
                    1 == < WallClock as wasmtime::component::ComponentType >::ALIGN32
                );
            };
<<<<<<< HEAD
            #[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
            pub trait Host: Send {}
            impl<_T: Host + Send> Host for &mut _T {}
            pub fn add_to_linker<T, D>(
||||||| 40315bd2c
            pub trait Host {}
            pub trait GetHost<
                T,
                D,
            >: Fn(T) -> <Self as GetHost<T, D>>::Host + Send + Sync + Copy + 'static {
                type Host: Host + Send;
            }
            impl<F, T, D, O> GetHost<T, D> for F
            where
                F: Fn(T) -> O + Send + Sync + Copy + 'static,
                O: Host + Send,
            {
                type Host = O;
            }
            pub fn add_to_linker_get_host<
                T,
                G: for<'a> GetHost<&'a mut T, T, Host: Host + Send>,
            >(
=======
            pub trait Host {}
            pub fn add_to_linker_get_host<T, G>(
>>>>>>> upstream/main
                linker: &mut wasmtime::component::Linker<T>,
                host_getter: fn(&mut T) -> D::Data<'_>,
            ) -> wasmtime::Result<()>
            where
<<<<<<< HEAD
                D: wasmtime::component::HasData,
                for<'a> D::Data<'a>: Host,
                T: 'static + Send,
||||||| 40315bd2c
                T: Send + 'static,
=======
                T: 'static,
                G: for<'a> wasmtime::component::GetHost<&'a mut T, Host: Host + Send>,
                T: Send + 'static,
>>>>>>> upstream/main
            {
                let mut inst = linker.instance("foo:foo/wall-clock")?;
                Ok(())
            }
<<<<<<< HEAD
||||||| 40315bd2c
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
            impl<_T: Host + ?Sized> Host for &mut _T {}
=======
            pub fn add_to_linker<T, U>(
                linker: &mut wasmtime::component::Linker<T>,
                get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
            ) -> wasmtime::Result<()>
            where
                T: 'static,
                U: Host + Send,
                T: Send + 'static,
            {
                add_to_linker_get_host(linker, get)
            }
            impl<_T: Host + ?Sized> Host for &mut _T {}
>>>>>>> upstream/main
        }
    }
}
