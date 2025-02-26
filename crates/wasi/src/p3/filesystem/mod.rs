use wasmtime::component::{Linker, ResourceTable};

use crate::p3::ResourceView;

mod host;

#[repr(transparent)]
pub struct WasiFilesystemImpl<T>(pub T);

impl<T: WasiFilesystemView> WasiFilesystemView for &mut T {
    fn filesystem(&mut self) -> &mut WasiFilesystemCtx {
        (**self).filesystem()
    }
}

impl<T: WasiFilesystemView> WasiFilesystemView for WasiFilesystemImpl<T> {
    fn filesystem(&mut self) -> &mut WasiFilesystemCtx {
        self.0.filesystem()
    }
}

impl<T: ResourceView> ResourceView for WasiFilesystemImpl<T> {
    fn table(&mut self) -> &mut ResourceTable {
        self.0.table()
    }
}

pub trait WasiFilesystemView: ResourceView + Send {
    fn filesystem(&mut self) -> &mut WasiFilesystemCtx;
}

#[derive(Default)]
pub struct WasiFilesystemCtx {}

/// Add all WASI interfaces from this module into the `linker` provided.
///
/// This function will add the `async` variant of all interfaces into the
/// [`Linker`] provided. By `async` this means that this function is only
/// compatible with [`Config::async_support(true)`][async]. For embeddings with
/// async support disabled see [`add_to_linker_sync`] instead.
///
/// This function will add all interfaces implemented by this crate to the
/// [`Linker`], which corresponds to the `wasi:filesystem/imports` world supported by
/// this crate.
///
/// [async]: wasmtime::Config::async_support
///
/// # Example
///
/// ```
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker};
/// use wasmtime_wasi::p3::filesystem::{WasiFilesystemView, WasiFilesystemCtx};
/// use wasmtime_wasi::p3::ResourceView;
///
/// fn main() -> Result<()> {
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p3::filesystem::add_to_linker(&mut linker)?;
///     // ... add any further functionality to `linker` if desired ...
///
///     let mut store = Store::new(
///         &engine,
///         MyState {
///             filesystem: WasiFilesystemCtx::default(),
///             table: ResourceTable::default(),
///         },
///     );
///
///     // ... use `linker` to instantiate within `store` ...
///
///     Ok(())
/// }
///
/// struct MyState {
///     filesystem: WasiFilesystemCtx,
///     table: ResourceTable,
/// }
///
/// impl ResourceView for MyState {
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
///
/// impl WasiFilesystemView for MyState {
///     fn filesystem(&mut self) -> &mut WasiFilesystemCtx { &mut self.filesystem }
/// }
/// ```
pub fn add_to_linker<T: WasiFilesystemView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    let closure = annotate_filesystem(|cx| WasiFilesystemImpl(cx));
    crate::p3::bindings::filesystem::types::add_to_linker_get_host(linker, closure)?;
    crate::p3::bindings::filesystem::preopens::add_to_linker_get_host(linker, closure)?;
    Ok(())
}

fn annotate_filesystem<T, F>(val: F) -> F
where
    F: Fn(&mut T) -> WasiFilesystemImpl<&mut T>,
{
    val
}
