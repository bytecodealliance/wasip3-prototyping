use crate::p3::ResourceView;
use core::future::Future;
use core::net::SocketAddr;
use core::ops::Deref;
use core::pin::Pin;
use std::sync::Arc;
use wasmtime::component::{HasData, Linker, ResourceTable};

mod host;
pub mod tcp;
pub mod udp;
pub mod util;

#[repr(transparent)]
pub struct WasiSocketsImpl<T>(pub T);

impl<T: WasiSocketsView> WasiSocketsView for &mut T {
    fn sockets(&self) -> &WasiSocketsCtx {
        (**self).sockets()
    }
}

impl<T: WasiSocketsView> WasiSocketsView for WasiSocketsImpl<T> {
    fn sockets(&self) -> &WasiSocketsCtx {
        self.0.sockets()
    }
}

impl<T: ResourceView> ResourceView for WasiSocketsImpl<T> {
    fn table(&mut self) -> &mut ResourceTable {
        self.0.table()
    }
}

pub trait WasiSocketsView: ResourceView + Send {
    fn sockets(&self) -> &WasiSocketsCtx;
}

#[derive(Clone, Default)]
pub struct WasiSocketsCtx {
    pub socket_addr_check: SocketAddrCheck,
    pub allowed_network_uses: AllowedNetworkUses,
}

pub struct Network {
    pub socket_addr_check: SocketAddrCheck,
    pub allow_ip_name_lookup: bool,
}

/// A check that will be called for each socket address that is used of whether the address is permitted.
#[derive(Clone)]
pub struct SocketAddrCheck(
    pub(crate)  Arc<
        dyn Fn(SocketAddr, SocketAddrUse) -> Pin<Box<dyn Future<Output = bool> + Send + Sync>>
            + Send
            + Sync,
    >,
);

impl SocketAddrCheck {
    /// A check that will be called for each socket address that is used.
    ///
    /// Returning `true` will permit socket connections to the `SocketAddr`,
    /// while returning `false` will reject the connection.
    pub fn new(
        f: impl Fn(SocketAddr, SocketAddrUse) -> Pin<Box<dyn Future<Output = bool> + Send + Sync>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self(Arc::new(f))
    }

    pub async fn check(&self, addr: SocketAddr, reason: SocketAddrUse) -> std::io::Result<()> {
        if (self.0)(addr, reason).await {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "An address was not permitted by the socket address check.",
            ))
        }
    }
}

impl Deref for SocketAddrCheck {
    type Target = dyn Fn(SocketAddr, SocketAddrUse) -> Pin<Box<dyn Future<Output = bool> + Send + Sync>>
        + Send
        + Sync;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl Default for SocketAddrCheck {
    fn default() -> Self {
        Self(Arc::new(|_, _| Box::pin(async { false })))
    }
}

/// The reason what a socket address is being used for.
#[derive(Clone, Copy, Debug)]
pub enum SocketAddrUse {
    /// Binding TCP socket
    TcpBind,
    /// Connecting TCP socket
    TcpConnect,
    /// Binding UDP socket
    UdpBind,
    /// Connecting UDP socket
    UdpConnect,
    /// Sending datagram on non-connected UDP socket
    UdpOutgoingDatagram,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SocketAddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Copy, Clone)]
pub struct AllowedNetworkUses {
    pub ip_name_lookup: bool,
    pub udp: bool,
    pub tcp: bool,
}

impl Default for AllowedNetworkUses {
    fn default() -> Self {
        Self {
            ip_name_lookup: false,
            udp: true,
            tcp: true,
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
/// [`Linker`], which corresponds to the `wasi:sockets/imports` world supported by
/// this crate.
///
/// [async]: wasmtime::Config::async_support
///
/// # Example
///
/// ```
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker};
/// use wasmtime_wasi::p3::sockets::{WasiSocketsView, WasiSocketsCtx};
/// use wasmtime_wasi::p3::ResourceView;
///
/// fn main() -> Result<()> {
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p3::sockets::add_to_linker(&mut linker)?;
///     // ... add any further functionality to `linker` if desired ...
///
///     let mut store = Store::new(
///         &engine,
///         MyState {
///             sockets: WasiSocketsCtx::default(),
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
///     sockets: WasiSocketsCtx,
///     table: ResourceTable,
/// }
///
/// impl ResourceView for MyState {
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
///
/// impl WasiSocketsView for MyState {
///     fn sockets(&self) -> &WasiSocketsCtx { &self.sockets }
/// }
/// ```
pub fn add_to_linker<T: WasiSocketsView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    crate::p3::bindings::sockets::types::add_to_linker::<_, WasiSockets<T>>(linker, |x| {
        WasiSocketsImpl(x)
    })?;
    crate::p3::bindings::sockets::ip_name_lookup::add_to_linker::<_, WasiSockets<T>>(
        linker,
        |x| WasiSocketsImpl(x),
    )?;
    Ok(())
}

struct WasiSockets<T>(T);

impl<T: 'static> HasData for WasiSockets<T> {
    type Data<'a> = WasiSocketsImpl<&'a mut T>;
}
