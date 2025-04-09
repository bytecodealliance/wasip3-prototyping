mod host;

use wasmtime::component::Linker;

use crate::clocks::host::{monotonic_clock, wall_clock};
use crate::clocks::{HostMonotonicClock, HostWallClock};

#[repr(transparent)]
pub struct WasiClocksImpl<T>(pub T);

impl<T: WasiClocksView> WasiClocksView for &mut T {
    fn clocks(&mut self) -> &WasiClocksCtx {
        (**self).clocks()
    }
}

impl<T: WasiClocksView> WasiClocksView for WasiClocksImpl<T> {
    fn clocks(&mut self) -> &WasiClocksCtx {
        self.0.clocks()
    }
}

pub trait WasiClocksView: Send {
    fn clocks(&mut self) -> &WasiClocksCtx;
}

pub struct WasiClocksCtx {
    pub wall_clock: Box<dyn HostWallClock + Send>,
    pub monotonic_clock: Box<dyn HostMonotonicClock + Send>,
}

impl Default for WasiClocksCtx {
    fn default() -> Self {
        Self {
            wall_clock: wall_clock(),
            monotonic_clock: monotonic_clock(),
        }
    }
}

/// Add all WASI interfaces from this module into the `linker` provided.
///
/// This function will add the `async` variant of all interfaces into the
/// [`Linker`] provided. By `async` this means that this function is only
/// compatible with [`Config::async_support(true)`][async]. For embeddings with
/// async support disabled see [`add_to_linker_sync`] instead.
///
/// This function will add all interfaces implemented by this crate to the
/// [`Linker`], which corresponds to the `wasi:clocks/imports` world supported by
/// this crate.
///
/// [async]: wasmtime::Config::async_support
///
/// # Example
///
/// ```
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker};
/// use wasmtime_wasi::p3::clocks::{WasiClocksView, WasiClocksCtx};
///
/// fn main() -> Result<()> {
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p3::clocks::add_to_linker(&mut linker)?;
///     // ... add any further functionality to `linker` if desired ...
///
///     let mut store = Store::new(
///         &engine,
///         MyState {
///             clocks: WasiClocksCtx::default(),
///         },
///     );
///
///     // ... use `linker` to instantiate within `store` ...
///
///     Ok(())
/// }
///
/// struct MyState {
///     clocks: WasiClocksCtx,
/// }
///
/// impl WasiClocksView for MyState {
///     fn clocks(&mut self) -> &WasiClocksCtx { &self.clocks }
/// }
/// ```
pub fn add_to_linker<T: WasiClocksView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    let closure = annotate_clocks(|cx| WasiClocksImpl(cx));
    crate::p3::bindings::clocks::wall_clock::add_to_linker_get_host(linker, closure)?;
    crate::p3::bindings::clocks::monotonic_clock::add_to_linker_get_host(linker, closure)?;
    Ok(())
}

fn annotate_clocks<T, F>(val: F) -> F
where
    F: Fn(&mut T) -> WasiClocksImpl<&mut T>,
{
    val
}
