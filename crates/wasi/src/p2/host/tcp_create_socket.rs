use crate::p2::bindings::{sockets::network::IpAddressFamily, sockets::tcp_create_socket};
use crate::p2::tcp::TcpSocket;
use crate::p2::{IoView, SocketResult, WasiImpl, WasiP2View};
use wasmtime::component::Resource;

impl<T> tcp_create_socket::Host for WasiImpl<T>
where
    T: WasiP2View,
{
    fn create_tcp_socket(
        &mut self,
        address_family: IpAddressFamily,
    ) -> SocketResult<Resource<TcpSocket>> {
        let socket = TcpSocket::new(address_family.into())?;
        let socket = self.table().push(socket)?;
        Ok(socket)
    }
}
