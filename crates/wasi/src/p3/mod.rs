pub mod bindings;
pub mod cli;
pub mod clocks;
pub mod random;
pub mod sockets;
//pub mod filesystem;

/// Add all WASI interfaces from this module into the `linker` provided.
///
/// This function will add the `async` variant of all interfaces into the
/// [`Linker`] provided. By `async` this means that this function is only
/// compatible with [`Config::async_support(true)`][async]. For embeddings with
/// async support disabled see [`add_to_linker_sync`] instead.
///
/// This function will add all interfaces implemented by this crate to the
/// [`Linker`], which corresponds to the `wasi:cli/imports` world supported by
/// this crate.
///
/// [async]: wasmtime::Config::async_support
///
/// # Example
///
/// ```
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker};
/// use wasmtime_wasi::p3::cli::{WasiCliCtx, WasiCliView};
/// use wasmtime_wasi::p3::clocks::{WasiClocksCtx, WasiClocksView};
/// use wasmtime_wasi::p3::random::{WasiRandomCtx, WasiRandomView};
/// use wasmtime_wasi::p3::sockets::{WasiSocketsCtx, WasiSocketsView};
///
/// fn main() -> Result<()> {
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p3::add_to_linker(&mut linker)?;
///     // ... add any further functionality to `linker` if desired ...
///
///     let mut store = Store::new(
///         &engine,
///         MyState::default(),
///     );
///
///     // ... use `linker` to instantiate within `store` ...
///
///     Ok(())
/// }
///
/// #[derive(Default)]
/// struct MyState {
///     cli: WasiCliCtx,
///     clocks: WasiClocksCtx,
///     random: WasiRandomCtx,
///     sockets: WasiSocketsCtx,
///     table: ResourceTable,
/// }
///
/// impl WasiCliView for MyState {
///     fn cli(&self) -> &WasiCliCtx { &self.cli }
/// }
///
/// impl WasiClocksView for MyState {
///     fn clocks(&self) -> &WasiClocksCtx { &self.clocks }
/// }
///
/// impl WasiRandomView for MyState {
///     fn random(&mut self) -> &mut WasiRandomCtx { &mut self.random }
/// }
///
/// impl WasiSocketsView for MyState {
///     fn sockets(&self) -> &WasiSocketsCtx { &self.sockets }
///
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
/// ```
pub fn add_to_linker<T>(linker: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
where
    T: clocks::WasiClocksView
        + random::WasiRandomView
        + sockets::WasiSocketsView
        //+ filesystem::WasiFilesystemView
        + cli::WasiCliView
        + 'static,
{
    clocks::add_to_linker(linker)?;
    random::add_to_linker(linker)?;
    sockets::add_to_linker(linker)?;
    //filesystem::add_to_linker(linker)?;
    cli::add_to_linker(linker)?;
    Ok(())
}
