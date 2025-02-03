//! Auto-generated bindings for `wasi-clocks`
//!
//! This module contains the output of the [`bindgen!`] macro when run over
//! the `wasi:clocks/imports` world.
//!
//! [`bindgen!`]: https://docs.rs/wasmtime/latest/wasmtime/component/macro.bindgen.html
//!
//! # Examples
//!
//! If you have a WIT world which refers to `wasi:clocks` interfaces you probably want to
//! use this crate's bindings rather than generate fresh bindings. That can be
//! done using the `with` option to [`bindgen!`]:
//!
//! ```rust
//! use core::future::Future;
//!
//! use wasmtime_wasi_clocks::{WasiClocksCtx, WasiClocksView};
//! use wasmtime::{Result, StoreContextMut, Engine, Config};
//! use wasmtime::component::{for_any, Linker};
//!
//! wasmtime::component::bindgen!({
//!     world: "example:wasi/my-world",
//!     inline: "
//!         package example:wasi;
//!
//!         // An example of extending the `wasi:clocks/imports` world with a
//!         // custom host interface.
//!         world my-world {
//!             include wasi:clocks/imports@0.3.0;
//!
//!             import custom-host;
//!         }
//!
//!         interface custom-host {
//!             my-custom-function: func();
//!         }
//!     ",
//!     path: "src/p3/wit",
//!     with: {
//!         "wasi:clocks": wasmtime_wasi_clocks::p3::bindings,
//!     },
//!     concurrent_imports: true,
//!     async: {
//!         only_imports: [
//!             "example:wasi/custom-host#my-custom-function",
//!             "wasi:clocks/monotonic-clock@0.3.0#wait-for",
//!             "wasi:clocks/monotonic-clock@0.3.0#wait-until",
//!         ],
//!     },
//! });
//!
//! struct MyState {
//!     clocks: WasiClocksCtx,
//! }
//!
//! impl example::wasi::custom_host::Host for MyState {
//!     type Data = Self;
//!
//!     fn my_custom_function(
//!        store: StoreContextMut<'_, Self::Data>,
//!     ) -> impl Future<
//!         Output = impl FnOnce(StoreContextMut<'_, Self::Data>) + 'static
//!     > + 'static {
//!         async move {
//!             // ..
//!             for_any(|_| {})
//!         }
//!     }
//! }
//!
//! impl WasiClocksView for MyState {
//!     fn clocks(&self) -> &WasiClocksCtx { &self.clocks }
//! }
//!
//! fn main() -> Result<()> {
//!     let mut config = Config::default();
//!     config.async_support(true);
//!     let engine = Engine::new(&config)?;
//!     let mut linker: Linker<MyState> = Linker::new(&engine);
//!     wasmtime_wasi_clocks::p3::add_to_linker(&mut linker)?;
//!     //example::wasi::custom_host::add_to_linker(&mut linker, |state| state)?;
//!
//!     // .. use `Linker` to instantiate component ...
//!
//!     Ok(())
//! }
//! ```

mod generated {
    wasmtime::component::bindgen!({
        path: "src/p3/wit",
        world: "wasi:clocks/imports",
        tracing: true,
        trappable_imports: true,
        concurrent_imports: true,
        async: {
            only_imports: [
                "wasi:clocks/monotonic-clock@0.3.0#wait-for",
                "wasi:clocks/monotonic-clock@0.3.0#wait-until",
            ],
        },
    });
}
pub use self::generated::wasi::clocks::*;
