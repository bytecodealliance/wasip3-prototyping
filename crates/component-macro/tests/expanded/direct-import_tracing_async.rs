/// Auto-generated bindings for a pre-instantiated version of a
/// component which implements the world `foo`.
///
/// This structure is created through [`FooPre::new`] which
/// takes a [`InstancePre`](wasmtime::component::InstancePre) that
/// has been created through a [`Linker`](wasmtime::component::Linker).
///
/// For more information see [`Foo`] as well.
pub struct FooPre<T: 'static> {
    instance_pre: wasmtime::component::InstancePre<T>,
    indices: FooIndices,
}
impl<T: 'static> Clone for FooPre<T> {
    fn clone(&self) -> Self {
        Self {
            instance_pre: self.instance_pre.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl<_T: 'static> FooPre<_T> {
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
#[wasmtime::component::__internal::trait_variant_make(::core::marker::Send)]
pub trait FooImports: Send {
    async fn foo(&mut self) -> ();
}
<<<<<<< HEAD
impl<_T: FooImports + Send> FooImports for &mut _T {
||||||| 40315bd2c
pub trait FooImportsGetHost<
    T,
    D,
>: Fn(T) -> <Self as FooImportsGetHost<T, D>>::Host + Send + Sync + Copy + 'static {
    type Host: FooImports;
}
impl<F, T, D, O> FooImportsGetHost<T, D> for F
where
    F: Fn(T) -> O + Send + Sync + Copy + 'static,
    O: FooImports,
{
    type Host = O;
}
impl<_T: FooImports + ?Sized + Send> FooImports for &mut _T {
=======
impl<_T: FooImports + ?Sized + Send> FooImports for &mut _T {
>>>>>>> upstream/main
    async fn foo(&mut self) -> () {
        FooImports::foo(*self).await
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
        pub async fn instantiate_async<_T: 'static>(
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
        pub fn add_to_linker_imports<T, D>(
            linker: &mut wasmtime::component::Linker<T>,
            host_getter: fn(&mut T) -> D::Data<'_>,
        ) -> wasmtime::Result<()>
        where
            D: wasmtime::component::HasData,
            for<'a> D::Data<'a>: FooImports,
<<<<<<< HEAD
            T: 'static + Send,
||||||| 40315bd2c
            T: Send,
=======
            T: Send + 'static,
>>>>>>> upstream/main
        {
            let mut linker = linker.root();
            linker
                .func_wrap_async(
                    "foo",
                    move |mut caller: wasmtime::StoreContextMut<'_, T>, (): ()| {
                        use tracing::Instrument;
                        let span = tracing::span!(
                            tracing::Level::TRACE, "wit-bindgen import", module = "foo",
                            function = "foo",
                        );
                        wasmtime::component::__internal::Box::new(
                            async move {
                                tracing::event!(tracing::Level::TRACE, "call");
                                let host = &mut host_getter(caller.data_mut());
                                let r = FooImports::foo(host).await;
                                tracing::event!(
                                    tracing::Level::TRACE, result = tracing::field::debug(& r),
                                    "return"
                                );
                                Ok(r)
                            }
                                .instrument(span),
                        )
                    },
                )?;
            Ok(())
        }
        pub fn add_to_linker<T, D>(
            linker: &mut wasmtime::component::Linker<T>,
<<<<<<< HEAD
            host_getter: fn(&mut T) -> D::Data<'_>,
||||||| 40315bd2c
            get: impl Fn(&mut T) -> &mut U + Send + Sync + Copy + 'static,
=======
            get: fn(&mut T) -> D::Data<'_>,
>>>>>>> upstream/main
        ) -> wasmtime::Result<()>
        where
            D: wasmtime::component::HasData,
            for<'a> D::Data<'a>: FooImports + Send,
<<<<<<< HEAD
            T: 'static + Send,
||||||| 40315bd2c
            T: Send,
            U: FooImports + Send,
=======
            T: Send + 'static,
>>>>>>> upstream/main
        {
<<<<<<< HEAD
            Self::add_to_linker_imports::<T, D>(linker, host_getter)?;
||||||| 40315bd2c
            Self::add_to_linker_imports_get_host(linker, get)?;
=======
            Self::add_to_linker_imports::<T, D>(linker, get)?;
>>>>>>> upstream/main
            Ok(())
        }
    }
};
