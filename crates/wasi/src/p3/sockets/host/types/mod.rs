use core::net::SocketAddr;

use wasmtime::component::Accessor;

use crate::p3::bindings::sockets::types::Host;
use crate::p3::sockets::{SocketAddrCheck, SocketAddrUse, WasiSocketsImpl, WasiSocketsView};

mod tcp;
mod udp;

impl<T> Host for WasiSocketsImpl<&mut T> where T: WasiSocketsView + 'static {}

fn get_socket_addr_check<T, U>(store: &mut Accessor<T, U>) -> SocketAddrCheck
where
    U: WasiSocketsView,
{
    store.with(|view| view.sockets().socket_addr_check.clone())
}

async fn is_addr_allowed<T, U>(
    store: &mut Accessor<T, U>,
    addr: SocketAddr,
    reason: SocketAddrUse,
) -> bool
where
    U: WasiSocketsView,
{
    get_socket_addr_check(store)(addr, reason).await
}
