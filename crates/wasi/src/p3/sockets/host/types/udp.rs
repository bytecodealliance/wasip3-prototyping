#![allow(unused)] // TODO: Remove

use wasmtime::component::Resource;

use crate::p3::bindings::sockets::types::{
    ErrorCode, HostUdpSocket, IpAddressFamily, IpSocketAddress, UdpSocket,
};
use crate::p3::sockets::{WasiSocketsImpl, WasiSocketsView};

impl<T> HostUdpSocket for WasiSocketsImpl<T>
where
    T: WasiSocketsView,
{
    fn new(&mut self, address_family: IpAddressFamily) -> wasmtime::Result<Resource<UdpSocket>> {
        todo!()
    }

    fn bind(
        &mut self,
        self_: Resource<UdpSocket>,
        local_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn connect(
        &mut self,
        self_: Resource<UdpSocket>,
        remote_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn disconnect(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn send(
        &mut self,
        self_: Resource<UdpSocket>,
        data: Vec<u8>,
        remote_address: Option<IpSocketAddress>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn receive(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<(Vec<u8>, IpSocketAddress), ErrorCode>> {
        todo!()
    }

    fn local_address(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        todo!()
    }

    fn remote_address(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        todo!()
    }

    fn address_family(&mut self, self_: Resource<UdpSocket>) -> wasmtime::Result<IpAddressFamily> {
        todo!()
    }

    fn unicast_hop_limit(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<u8, ErrorCode>> {
        todo!()
    }

    fn set_unicast_hop_limit(
        &mut self,
        self_: Resource<UdpSocket>,
        value: u8,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn receive_buffer_size(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        todo!()
    }

    fn set_receive_buffer_size(
        &mut self,
        self_: Resource<UdpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn send_buffer_size(
        &mut self,
        self_: Resource<UdpSocket>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        todo!()
    }

    fn set_send_buffer_size(
        &mut self,
        self_: Resource<UdpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn drop(&mut self, rep: Resource<UdpSocket>) -> wasmtime::Result<()> {
        todo!()
    }
}
