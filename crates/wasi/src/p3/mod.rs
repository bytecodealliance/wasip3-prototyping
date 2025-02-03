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
/// use wasmtime_wasi_cli::{WasiCliView, WasiCliCtx};
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
///         MyState {
///             cli: WasiCliCtx::default(),
///         },
///     );
///
///     // ... use `linker` to instantiate within `store` ...
///
///     Ok(())
/// }
///
/// struct MyState {
///     cli: WasiCliCtx,
/// }
///
/// impl wasmtime_wasi_cli::WasiCliView for MyState {
///     fn cli(&self) -> &WasiCliCtx { &self.cli }
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
