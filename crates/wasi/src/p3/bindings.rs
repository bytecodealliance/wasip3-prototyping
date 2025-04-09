//! Auto-generated bindings for WASI interfaces.
//!
//! This module contains the output of the [`bindgen!`] macro when run over
//! the `wasi:cli/imports` world.
//!
//! [`bindgen!`]: https://docs.rs/wasmtime/latest/wasmtime/component/macro.bindgen.html
//!
//! # Examples
//!
//! If you have a WIT world which refers to `wasi:cli` interfaces you probably want to
//! use this crate's bindings rather than generate fresh bindings. That can be
//! done using the `with` option to [`bindgen!`]:
//!
//! ```rust
//! use core::future::Future;
//!
//! use wasmtime_wasi::p3::cli::{WasiCliCtx, WasiCliView};
//! use wasmtime_wasi::p3::clocks::{WasiClocksCtx, WasiClocksView};
//! use wasmtime_wasi::p3::filesystem::{WasiFilesystemCtx, WasiFilesystemView};
//! use wasmtime_wasi::p3::random::{WasiRandomCtx, WasiRandomView};
//! use wasmtime_wasi::p3::sockets::{WasiSocketsCtx, WasiSocketsView};
//! use wasmtime_wasi::p3::ResourceView;
//! use wasmtime::{Result, StoreContextMut, Engine, Config};
//! use wasmtime::component::{Accessor, Linker, ResourceTable};
//!
//! wasmtime::component::bindgen!({
//!     world: "example:wasi/my-world",
//!     inline: "
//!         package example:wasi;
//!
//!         // An example of extending the `wasi:cli/imports` world with a
//!         // custom host interface.
//!         world my-world {
//!             include wasi:cli/imports@0.3.0;
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
//!         "wasi": wasmtime_wasi::p3::bindings,
//!     },
//!     concurrent_exports: true,
//!     concurrent_imports: true,
//!     async: {
//!         only_imports: [
//!             "example:wasi/custom-host#my-custom-function",
//!             "wasi:cli/stdin@0.3.0#get-stdin",
//!             "wasi:cli/stdout@0.3.0#set-stdout",
//!             "wasi:cli/stderr@0.3.0#set-stderr",
//!             "wasi:clocks/monotonic-clock@0.3.0#wait-for",
//!             "wasi:clocks/monotonic-clock@0.3.0#wait-until",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.read-via-stream",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.write-via-stream",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.append-via-stream",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.advise",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.sync-data",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.get-flags",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.get-type",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.set-size",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.set-times",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.read-directory",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.sync",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.create-directory-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.stat",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.stat-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.set-times-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.link-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.open-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.readlink-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.remove-directory-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.rename-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.symlink-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.unlink-file-at",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.is-same-object",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.metadata-hash",
//!             "wasi:filesystem/types@0.3.0#[method]descriptor.metadata-hash-at",
//!             "wasi:sockets/ip-name-lookup@0.3.0#resolve-addresses",
//!             "wasi:sockets/types@0.3.0#[method]tcp-socket.bind",
//!             "wasi:sockets/types@0.3.0#[method]tcp-socket.connect",
//!             "wasi:sockets/types@0.3.0#[method]tcp-socket.listen",
//!             "wasi:sockets/types@0.3.0#[method]tcp-socket.receive",
//!             "wasi:sockets/types@0.3.0#[method]tcp-socket.send",
//!             "wasi:sockets/types@0.3.0#[method]udp-socket.bind",
//!             "wasi:sockets/types@0.3.0#[method]udp-socket.connect",
//!             "wasi:sockets/types@0.3.0#[method]udp-socket.receive",
//!             "wasi:sockets/types@0.3.0#[method]udp-socket.send",
//!         ],
//!     },
//! });
//!
//! struct MyState {
//!     cli: WasiCliCtx,
//!     clocks: WasiClocksCtx,
//!     filesystem: WasiFilesystemCtx,
//!     random: WasiRandomCtx,
//!     sockets: WasiSocketsCtx,
//!     table: ResourceTable,
//! }
//!
//! impl example::wasi::custom_host::Host for MyState {
//!     async fn my_custom_function<T>(_store: &mut Accessor<T, Self>) {
//!         // ..
//!     }
//! }
//!
//! impl ResourceView for MyState {
//!     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
//! }
//!
//! impl WasiCliView for MyState {
//!     fn cli(&mut self) -> &WasiCliCtx { &self.cli }
//! }
//!
//! impl WasiClocksView for MyState {
//!     fn clocks(&mut self) -> &WasiClocksCtx { &self.clocks }
//! }
//!
//! impl WasiFilesystemView for MyState {
//!     fn filesystem(&self) -> &WasiFilesystemCtx { &self.filesystem }
//! }
//!
//! impl WasiRandomView for MyState {
//!     fn random(&mut self) -> &mut WasiRandomCtx { &mut self.random }
//! }
//!
//! impl WasiSocketsView for MyState {
//!     fn sockets(&self) -> &WasiSocketsCtx { &self.sockets }
//! }
//!
//! fn main() -> Result<()> {
//!     let mut config = Config::default();
//!     config.async_support(true);
//!     let engine = Engine::new(&config)?;
//!     let mut linker: Linker<MyState> = Linker::new(&engine);
//!     wasmtime_wasi::p3::add_to_linker(&mut linker)?;
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
        world: "wasi:cli/command",
        //tracing: true, // TODO: Re-enable once fixed
        trappable_imports: true,
        concurrent_exports: true,
        concurrent_imports: true,
        async: {
            only_imports: [
                "wasi:cli/stdin@0.3.0#get-stdin",
                "wasi:cli/stdout@0.3.0#set-stdout",
                "wasi:cli/stderr@0.3.0#set-stderr",
                "wasi:clocks/monotonic-clock@0.3.0#wait-for",
                "wasi:clocks/monotonic-clock@0.3.0#wait-until",
                "wasi:filesystem/types@0.3.0#[method]descriptor.read-via-stream",
                "wasi:filesystem/types@0.3.0#[method]descriptor.write-via-stream",
                "wasi:filesystem/types@0.3.0#[method]descriptor.append-via-stream",
                "wasi:filesystem/types@0.3.0#[method]descriptor.advise",
                "wasi:filesystem/types@0.3.0#[method]descriptor.sync-data",
                "wasi:filesystem/types@0.3.0#[method]descriptor.get-flags",
                "wasi:filesystem/types@0.3.0#[method]descriptor.get-type",
                "wasi:filesystem/types@0.3.0#[method]descriptor.set-size",
                "wasi:filesystem/types@0.3.0#[method]descriptor.set-times",
                "wasi:filesystem/types@0.3.0#[method]descriptor.read-directory",
                "wasi:filesystem/types@0.3.0#[method]descriptor.sync",
                "wasi:filesystem/types@0.3.0#[method]descriptor.create-directory-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.stat",
                "wasi:filesystem/types@0.3.0#[method]descriptor.stat-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.set-times-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.link-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.open-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.readlink-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.remove-directory-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.rename-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.symlink-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.unlink-file-at",
                "wasi:filesystem/types@0.3.0#[method]descriptor.is-same-object",
                "wasi:filesystem/types@0.3.0#[method]descriptor.metadata-hash",
                "wasi:filesystem/types@0.3.0#[method]descriptor.metadata-hash-at",
                "wasi:sockets/ip-name-lookup@0.3.0#resolve-addresses",
                "wasi:sockets/types@0.3.0#[method]tcp-socket.bind",
                "wasi:sockets/types@0.3.0#[method]tcp-socket.connect",
                "wasi:sockets/types@0.3.0#[method]tcp-socket.listen",
                "wasi:sockets/types@0.3.0#[method]tcp-socket.receive",
                "wasi:sockets/types@0.3.0#[method]tcp-socket.send",
                "wasi:sockets/types@0.3.0#[method]udp-socket.bind",
                "wasi:sockets/types@0.3.0#[method]udp-socket.connect",
                "wasi:sockets/types@0.3.0#[method]udp-socket.receive",
                "wasi:sockets/types@0.3.0#[method]udp-socket.send",
            ],
        },
        with: {
            "wasi:cli/terminal-input/terminal-input": crate::p3::cli::TerminalInput,
            "wasi:cli/terminal-output/terminal-output": crate::p3::cli::TerminalOutput,
            "wasi:filesystem/types/descriptor": crate::p3::filesystem::Descriptor,
            "wasi:sockets/types/tcp-socket": crate::p3::sockets::tcp::TcpSocket,
            "wasi:sockets/types/udp-socket": crate::p3::sockets::udp::UdpSocket,
        }
    });
}
pub use self::generated::exports;
pub use self::generated::wasi::*;
pub use self::generated::LinkOptions;

/// Bindings to execute and run a `wasi:cli/command`.
///
/// This structure is automatically generated by `bindgen!`.
///
/// This can be used for a more "typed" view of executing a command component
/// through the [`Command::wasi_cli_run`] method plus
/// [`Guest::call_run`](exports::wasi::cli::run::Guest::call_run).
///
/// # Examples
///
/// ```no_run
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{Component, Linker, ResourceTable};
/// use wasmtime_wasi::{IoView, WasiP2Ctx, WasiView, WasiP2CtxBuilder};
/// use wasmtime_wasi::bindings::Command;
///
/// // This example is an example shim of executing a component based on the
/// // command line arguments provided to this program.
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let args = std::env::args().skip(1).collect::<Vec<_>>();
///
///     // Configure and create `Engine`
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     // Configure a `Linker` with WASI, compile a component based on
///     // command line arguments, and then pre-instantiate it.
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
///     let component = Component::from_file(&engine, &args[0])?;
///
///
///     // Configure a `WasiP2Ctx` based on this program's environment. Then
///     // build a `Store` to instantiate into.
///     let mut builder = WasiP2CtxBuilder::new();
///     builder.inherit_stdio().inherit_env().args(&args);
///     let mut store = Store::new(
///         &engine,
///         MyState {
///             ctx: builder.build(),
///             table: ResourceTable::new(),
///         },
///     );
///
///     // Instantiate the component and we're off to the races.
///     let command = Command::instantiate_async(&mut store, &component, &linker).await?;
///     let program_result = command.wasi_cli_run().call_run(&mut store).await?;
///     match program_result {
///         Ok(()) => Ok(()),
///         Err(()) => std::process::exit(1),
///     }
/// }
///
/// struct MyState {
///     ctx: WasiP2Ctx,
///     table: ResourceTable,
/// }
///
/// impl IoView for MyState {
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
/// impl WasiView for MyState {
///     fn ctx(&mut self) -> &mut WasiP2Ctx { &mut self.ctx }
/// }
/// ```
///
/// ---
pub use self::generated::Command;

/// Pre-instantiated analog of [`Command`]
///
/// This can be used to front-load work such as export lookup before
/// instantiation.
///
/// # Examples
///
/// ```no_run
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker, Component};
/// use wasmtime_wasi::{IoView, WasiP2Ctx, WasiView, WasiP2CtxBuilder};
/// use wasmtime_wasi::bindings::CommandPre;
///
/// // This example is an example shim of executing a component based on the
/// // command line arguments provided to this program.
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let args = std::env::args().skip(1).collect::<Vec<_>>();
///
///     // Configure and create `Engine`
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     // Configure a `Linker` with WASI, compile a component based on
///     // command line arguments, and then pre-instantiate it.
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
///     let component = Component::from_file(&engine, &args[0])?;
///     let pre = CommandPre::new(linker.instantiate_pre(&component)?)?;
///
///
///     // Configure a `WasiP2Ctx` based on this program's environment. Then
///     // build a `Store` to instantiate into.
///     let mut builder = WasiP2CtxBuilder::new();
///     builder.inherit_stdio().inherit_env().args(&args);
///     let mut store = Store::new(
///         &engine,
///         MyState {
///             ctx: builder.build(),
///             table: ResourceTable::new(),
///         },
///     );
///
///     // Instantiate the component and we're off to the races.
///     let command = pre.instantiate_async(&mut store).await?;
///     let program_result = command.wasi_cli_run().call_run(&mut store).await?;
///     match program_result {
///         Ok(()) => Ok(()),
///         Err(()) => std::process::exit(1),
///     }
/// }
///
/// struct MyState {
///     ctx: WasiP2Ctx,
///     table: ResourceTable,
/// }
///
/// impl IoView for MyState {
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
/// impl WasiView for MyState {
///     fn ctx(&mut self) -> &mut WasiP2Ctx { &mut self.ctx }
/// }
/// ```
///
/// ---
pub use self::generated::CommandPre;

pub use self::generated::CommandIndices;
