use core::future::Future;
use core::mem;
use core::net::SocketAddr;

use anyhow::{ensure, Context as _};
use rustix::io::Errno;
use wasmtime::component::{for_any, FutureReader, Resource, StreamReader};
use wasmtime::StoreContextMut;

use crate::p3::bindings::sockets::types::{
    Duration, ErrorCode, HostTcpSocket, IpAddressFamily, IpSocketAddress, TcpSocket,
};
use crate::p3::sockets::tcp::{bind, connect, TcpState};
use crate::p3::sockets::{SocketAddrUse, WasiSocketsImpl, WasiSocketsView};

impl<T> HostTcpSocket for WasiSocketsImpl<&mut T>
where
    T: WasiSocketsView,
{
    type TcpSocketData = T;

    fn new(&mut self, address_family: IpAddressFamily) -> wasmtime::Result<Resource<TcpSocket>> {
        let socket = TcpSocket::new(address_family.into()).context("failed to create socket")?;
        let socket = self
            .table()
            .push(socket)
            .context("failed to push socket resource to table")?;
        Ok(socket)
    }

    fn bind(
        mut store: StoreContextMut<'_, Self::TcpSocketData>,
        mut socket: Resource<TcpSocket>,
        local_address: IpSocketAddress,
    ) -> impl Future<
        Output = impl FnOnce(
            StoreContextMut<'_, Self::TcpSocketData>,
        ) -> wasmtime::Result<Result<(), ErrorCode>>
                     + 'static,
    > + 'static {
        let ctx = store.data().sockets();
        let allowed = ctx.allowed_network_uses.tcp;
        let socket_addr_check = ctx.socket_addr_check.clone();
        let sock = store
            .data_mut()
            .table()
            .get_mut(&mut socket)
            .context("failed to get socket resource from table")
            .map(|socket| {
                let tcp_state = mem::replace(&mut socket.tcp_state, TcpState::BindStarted);
                if let TcpState::Default(sock) = tcp_state {
                    Some((sock, socket.family))
                } else {
                    socket.tcp_state = tcp_state;
                    None
                }
            });
        let local_address = SocketAddr::from(local_address);
        async move {
            let res = match sock {
                Ok(sock)
                    if !allowed
                        || !socket_addr_check(local_address, SocketAddrUse::TcpBind).await =>
                {
                    if let Some((sock, ..)) = sock {
                        Ok(Ok((sock, Err(ErrorCode::AccessDenied))))
                    } else {
                        Ok(Err(ErrorCode::AccessDenied))
                    }
                }
                Ok(Some((sock, family))) => {
                    let res = bind(&sock, local_address, family);
                    Ok(Ok((sock, res)))
                }
                Ok(None) => Ok(Err(ErrorCode::InvalidState)),
                Err(err) => Err(err),
            };
            for_any(move |mut store: StoreContextMut<'_, Self::TcpSocketData>| {
                let sock = res?;
                let socket = store
                    .data_mut()
                    .table()
                    .get_mut(&mut socket)
                    .context("failed to get socket resource from table")?;
                let (sock, res) = match sock {
                    Ok(sock) => sock,
                    Err(err) => return Ok(Err(err)),
                };
                ensure!(
                    matches!(socket.tcp_state, TcpState::BindStarted),
                    "corrupted socket state"
                );
                if let Err(err) = res {
                    socket.tcp_state = TcpState::Default(sock);
                    Ok(Err(err))
                } else {
                    socket.tcp_state = TcpState::Bound(sock);
                    Ok(Ok(()))
                }
            })
        }
    }

    fn connect(
        mut store: StoreContextMut<'_, Self::TcpSocketData>,
        mut socket: Resource<TcpSocket>,
        remote_address: IpSocketAddress,
    ) -> impl Future<
        Output = impl FnOnce(
            StoreContextMut<'_, Self::TcpSocketData>,
        ) -> wasmtime::Result<Result<(), ErrorCode>>
                     + 'static,
    > + 'static {
        let ctx = store.data().sockets();
        let allowed = ctx.allowed_network_uses.tcp;
        let socket_addr_check = ctx.socket_addr_check.clone();
        let sock = store
            .data_mut()
            .table()
            .get_mut(&mut socket)
            .context("failed to get socket resource from table")
            .map(|socket| {
                let tcp_state = mem::replace(&mut socket.tcp_state, TcpState::Connecting);
                if let TcpState::Default(sock) = tcp_state {
                    Some((sock, false, socket.family))
                } else if let TcpState::Bound(sock) = tcp_state {
                    Some((sock, true, socket.family))
                } else {
                    socket.tcp_state = tcp_state;
                    None
                }
            });
        let remote_address = SocketAddr::from(remote_address);
        async move {
            let res = match sock {
                Ok(sock)
                    if !allowed
                        || !socket_addr_check(remote_address, SocketAddrUse::TcpConnect).await =>
                {
                    if let Some((sock, bound, ..)) = sock {
                        Ok(Ok(Err((sock, bound, ErrorCode::AccessDenied))))
                    } else {
                        Ok(Err(ErrorCode::AccessDenied))
                    }
                }
                Ok(Some((sock, .., family))) => {
                    Ok(Ok(Ok(connect(sock, remote_address, family).await)))
                }
                Ok(None) => Ok(Err(ErrorCode::InvalidState)),
                Err(err) => Err(err),
            };
            for_any(move |mut store: StoreContextMut<'_, Self::TcpSocketData>| {
                let sock = res?;
                let socket = store
                    .data_mut()
                    .table()
                    .get_mut(&mut socket)
                    .context("failed to get socket resource from table")?;
                let sock = match sock {
                    Ok(sock) => sock,
                    Err(err) => return Ok(Err(err)),
                };
                ensure!(
                    matches!(socket.tcp_state, TcpState::Connecting),
                    "corrupted socket state"
                );
                match sock {
                    Ok(Ok(stream)) => {
                        socket.tcp_state = TcpState::Connected(stream);
                        Ok(Ok(()))
                    }
                    Ok(Err(err)) => {
                        socket.tcp_state = TcpState::Closed;
                        Ok(Err(err))
                    }
                    Err((sock, true, err)) => {
                        socket.tcp_state = TcpState::Bound(sock);
                        Ok(Err(err))
                    }
                    Err((sock, false, err)) => {
                        socket.tcp_state = TcpState::Default(sock);
                        Ok(Err(err))
                    }
                }
            })
        }
    }

    fn listen(
        &mut self,
        mut socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<StreamReader<Resource<TcpSocket>>, ErrorCode>> {
        let ctx = self.sockets();
        let allowed = ctx.allowed_network_uses.tcp;
        let socket = self
            .table()
            .get_mut(&mut socket)
            .context("failed to get socket resource from table")?;
        let sock = match mem::replace(&mut socket.tcp_state, TcpState::Closed) {
            TcpState::Default(sock) | TcpState::Bound(sock) => sock,
            tcp_state => {
                socket.tcp_state = tcp_state;
                return Ok(Err(ErrorCode::InvalidState));
            }
        };
        match sock.listen(socket.listen_backlog_size) {
            Ok(listener) => {
                socket.tcp_state = TcpState::Listening(listener);
                //let (tx, rx) = stream(self).context("failed to create stream")?;
                // TODO: Store the worker task in enum
                // TODO: Get a store/refactor
                //tx.write();
                //Ok(Ok(rx))
                Ok(Ok(todo!()))
            }
            Err(err) => {
                match Errno::from_io_error(&err) {
                    // See: https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-listen#:~:text=WSAEMFILE
                    // According to the docs, `listen` can return EMFILE on Windows.
                    // This is odd, because we're not trying to create a new socket
                    // or file descriptor of any kind. So we rewrite it to less
                    // surprising error code.
                    //
                    // At the time of writing, this behavior has never been experimentally
                    // observed by any of the wasmtime authors, so we're relying fully
                    // on Microsoft's documentation here.
                    #[cfg(windows)]
                    Some(Errno::MFILE) => Ok(Err(ErrorCode::OutOfMemory)),

                    _ => Ok(Err(err.into())),
                }
            }
        }
    }

    fn send(
        &mut self,
        socket: Resource<TcpSocket>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn receive(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<Result<(), ErrorCode>>)> {
        todo!()
    }

    fn local_address(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket resource from table")?;
        Ok(sock.local_address())
    }

    fn remote_address(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket resource from table")?;
        Ok(sock.remote_address())
    }

    fn is_listening(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<bool> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.is_listening())
    }

    fn address_family(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<IpAddressFamily> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.address_family())
    }

    fn set_listen_backlog_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get_mut(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_listen_backlog_size(value))
    }

    fn keep_alive_enabled(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<bool, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.keep_alive_enabled())
    }

    fn set_keep_alive_enabled(
        &mut self,
        socket: Resource<TcpSocket>,
        value: bool,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_keep_alive_enabled(value))
    }

    fn keep_alive_idle_time(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<Duration, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.keep_alive_idle_time())
    }

    fn set_keep_alive_idle_time(
        &mut self,
        socket: Resource<TcpSocket>,
        value: Duration,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get_mut(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_keep_alive_idle_time(value))
    }

    fn keep_alive_interval(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<Duration, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.keep_alive_interval())
    }

    fn set_keep_alive_interval(
        &mut self,
        socket: Resource<TcpSocket>,
        value: Duration,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_keep_alive_interval(value))
    }

    fn keep_alive_count(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u32, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.keep_alive_count())
    }

    fn set_keep_alive_count(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u32,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_keep_alive_count(value))
    }

    fn hop_limit(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u8, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.hop_limit())
    }

    fn set_hop_limit(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u8,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_hop_limit(value))
    }

    fn receive_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.receive_buffer_size())
    }

    fn set_receive_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get_mut(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_receive_buffer_size(value))
    }

    fn send_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        let sock = self
            .table()
            .get(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.send_buffer_size())
    }

    fn set_send_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = self
            .table()
            .get_mut(&socket)
            .context("failed to get socket from table")?;
        Ok(sock.set_send_buffer_size(value))
    }

    fn drop(&mut self, rep: Resource<TcpSocket>) -> wasmtime::Result<()> {
        self.table()
            .delete(rep)
            .context("failed to delete socket resource from table")?;
        Ok(())
    }
}
