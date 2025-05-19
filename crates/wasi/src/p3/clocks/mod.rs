mod host;

use crate::p3::bindings::clocks;
use cap_std::time::{Duration, Instant, SystemClock};
use cap_std::{AmbientAuthority, ambient_authority};
use cap_time_ext::{MonotonicClockExt as _, SystemClockExt as _};
use wasmtime::component::{HasData, Linker};

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

pub trait HostWallClock: Send {
    fn resolution(&self) -> Duration;
    fn now(&self) -> Duration;
}

pub trait HostMonotonicClock: Send {
    fn resolution(&self) -> u64;
    fn now(&self) -> u64;
}

pub struct WallClock {
    /// The underlying system clock.
    clock: cap_std::time::SystemClock,
}

impl Default for WallClock {
    fn default() -> Self {
        Self::new(ambient_authority())
    }
}

impl WallClock {
    pub fn new(ambient_authority: AmbientAuthority) -> Self {
        Self {
            clock: cap_std::time::SystemClock::new(ambient_authority),
        }
    }
}

impl HostWallClock for WallClock {
    fn resolution(&self) -> Duration {
        self.clock.resolution()
    }

    fn now(&self) -> Duration {
        // WASI defines wall clocks to return "Unix time".
        self.clock
            .now()
            .duration_since(SystemClock::UNIX_EPOCH)
            .unwrap()
    }
}

pub struct MonotonicClock {
    /// The underlying system clock.
    clock: cap_std::time::MonotonicClock,

    /// The `Instant` this clock was created. All returned times are
    /// durations since that time.
    initial: Instant,
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new(ambient_authority())
    }
}

impl MonotonicClock {
    pub fn new(ambient_authority: AmbientAuthority) -> Self {
        let clock = cap_std::time::MonotonicClock::new(ambient_authority);
        let initial = clock.now();
        Self { clock, initial }
    }
}

impl HostMonotonicClock for MonotonicClock {
    fn resolution(&self) -> u64 {
        self.clock.resolution().as_nanos().try_into().unwrap()
    }

    fn now(&self) -> u64 {
        // Unwrap here and in `resolution` above; a `u64` is wide enough to
        // hold over 584 years of nanoseconds.
        self.clock
            .now()
            .duration_since(self.initial)
            .as_nanos()
            .try_into()
            .unwrap()
    }
}

pub fn monotonic_clock() -> Box<dyn HostMonotonicClock + Send> {
    Box::new(MonotonicClock::default())
}

pub fn wall_clock() -> Box<dyn HostWallClock + Send> {
    Box::new(WallClock::default())
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
    clocks::wall_clock::add_to_linker::<_, WasiClocks<T>>(linker, |x| WasiClocksImpl(x))?;
    clocks::monotonic_clock::add_to_linker::<_, WasiClocks<T>>(linker, |x| WasiClocksImpl(x))?;
    Ok(())
}

struct WasiClocks<T>(T);

impl<T: 'static> HasData for WasiClocks<T> {
    type Data<'a> = WasiClocksImpl<&'a mut T>;
}
