use core::net::SocketAddr;

use wasmtime::component::Accessor;

use crate::p3::bindings::sockets::types::{Host, HostConcurrent};
use crate::p3::sockets::{
    SocketAddrCheck, SocketAddrUse, WasiSockets, WasiSocketsImpl, WasiSocketsView,
};

mod tcp;
mod udp;

impl<T> Host for WasiSocketsImpl<T> where T: WasiSocketsView {}

impl<T> HostConcurrent for WasiSockets<T> where T: WasiSocketsView + 'static {}

fn get_socket_addr_check<T, U>(store: &Accessor<T, WasiSockets<U>>) -> SocketAddrCheck
where
    U: WasiSocketsView + 'static,
{
    store.with(|mut view| view.get().sockets().socket_addr_check.clone())
}

async fn is_addr_allowed<T, U>(
    store: &Accessor<T, WasiSockets<U>>,
    addr: SocketAddr,
    reason: SocketAddrUse,
) -> bool
where
    U: WasiSocketsView + 'static,
{
    get_socket_addr_check(store)(addr, reason).await
}
