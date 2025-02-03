use core::future::{poll_fn, Future};
use core::mem;
use core::net::SocketAddr;
use core::pin::{pin, Pin};
use core::task::Poll;

use std::collections::VecDeque;
use std::io::IoSlice;
use std::net::Shutdown;
use std::sync::Arc;

use anyhow::{ensure, Context as _};
use io_lifetimes::AsSocketlike as _;
use rustix::io::Errno;
use tokio::io::AsyncWriteExt as _;
use tokio::spawn;
use wasmtime::component::{
    stream, Accessor, FutureReader, Lift, Resource, ResourceTable, StreamReader,
};

use crate::p3::bindings::sockets::types::{
    Duration, ErrorCode, HostTcpSocket, IpAddressFamily, IpSocketAddress, TcpSocket,
};
use crate::p3::sockets::tcp::{bind, TcpState};
use crate::p3::sockets::util::is_valid_unicast_address;
use crate::p3::sockets::{SocketAddrUse, WasiSocketsImpl, WasiSocketsView};
use crate::runtime::AbortOnDropJoinHandle;

fn is_tcp_allowed<T>(store: &mut Accessor<T>) -> bool
where
    T: WasiSocketsView,
{
    store.with(|store| store.data().sockets().allowed_network_uses.tcp)
}

async fn is_addr_allowed<T>(
    store: &mut Accessor<T>,
    addr: SocketAddr,
    reason: SocketAddrUse,
) -> bool
where
    T: WasiSocketsView,
{
    store
        .with(|store| {
            let socket_addr_check = store.data().sockets().socket_addr_check.clone();
            async move { socket_addr_check(addr, reason).await }
        })
        .await
}

fn get_socket<'a>(
    table: &'a ResourceTable,
    socket: &'a Resource<TcpSocket>,
) -> wasmtime::Result<&'a TcpSocket> {
    table
        .get(socket)
        .context("failed to get socket resource from table")
}

fn get_socket_mut<'a>(
    table: &'a mut ResourceTable,
    socket: &'a Resource<TcpSocket>,
) -> wasmtime::Result<&'a mut TcpSocket> {
    table
        .get_mut(socket)
        .context("failed to get socket resource from table")
}

fn next_item<T, U>(
    store: &mut Accessor<T>,
    stream: StreamReader<U>,
) -> wasmtime::Result<
    Pin<Box<dyn Future<Output = Option<(StreamReader<U>, Vec<U>)>> + Send + Sync + 'static>>,
>
where
    U: Send + Sync + Lift + 'static,
{
    let fut = store.with(|mut store| stream.read(&mut store).context("failed to read stream"))?;
    Ok(fut.into_future())
}

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

    async fn bind(
        store: &mut Accessor<Self::TcpSocketData>,
        socket: Resource<TcpSocket>,
        local_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let local_address = SocketAddr::from(local_address);
        if !is_tcp_allowed(store)
            || !is_addr_allowed(store, local_address, SocketAddrUse::TcpBind).await
        {
            return Ok(Err(ErrorCode::AccessDenied));
        }
        store.with(|mut store| {
            let socket = get_socket_mut(store.data_mut().table(), &socket)?;
            if !is_valid_unicast_address(local_address.ip(), socket.family) {
                return Ok(Err(ErrorCode::InvalidArgument));
            }
            match mem::replace(&mut socket.tcp_state, TcpState::Closed) {
                TcpState::Default(sock) => {
                    if let Err(err) = bind(&sock, local_address) {
                        socket.tcp_state = TcpState::Default(sock);
                        Ok(Err(err))
                    } else {
                        socket.tcp_state = TcpState::Bound(sock);
                        Ok(Ok(()))
                    }
                }
                tcp_state => {
                    socket.tcp_state = tcp_state;
                    Ok(Err(ErrorCode::InvalidState))
                }
            }
        })
    }

    async fn connect(
        store: &mut Accessor<Self::TcpSocketData>,
        socket: Resource<TcpSocket>,
        remote_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let remote_address = SocketAddr::from(remote_address);
        if !is_tcp_allowed(store)
            || !is_addr_allowed(store, remote_address, SocketAddrUse::TcpConnect).await
        {
            return Ok(Err(ErrorCode::AccessDenied));
        }
        let ip = remote_address.ip().to_canonical();
        if ip.is_unspecified() || remote_address.port() == 0 {
            return Ok(Err(ErrorCode::InvalidArgument));
        }

        match store.with(|mut store| {
            let socket = get_socket_mut(store.data_mut().table(), &socket)?;
            if !is_valid_unicast_address(ip, socket.family) {
                return Ok(Err(ErrorCode::InvalidArgument));
            }
            match mem::replace(&mut socket.tcp_state, TcpState::Connecting) {
                TcpState::Default(sock) | TcpState::Bound(sock) => Ok(Ok(sock)),
                tcp_state => {
                    socket.tcp_state = tcp_state;
                    Ok(Err(ErrorCode::InvalidState))
                }
            }
        }) {
            Ok(Ok(sock)) => {
                let res = sock.connect(remote_address).await;
                store.with(|mut store| {
                    let socket = get_socket_mut(store.data_mut().table(), &socket)?;
                    ensure!(
                        matches!(socket.tcp_state, TcpState::Connecting),
                        "corrupted socket state"
                    );
                    match res {
                        Ok(stream) => {
                            socket.tcp_state = TcpState::Connected(Arc::new(stream));
                            Ok(Ok(()))
                        }
                        Err(err) => {
                            socket.tcp_state = TcpState::Closed;
                            Ok(Err(err.into()))
                        }
                    }
                })
            }
            Ok(Err(err)) => Ok(Err(err)),
            Err(err) => Err(err),
        }
    }

    async fn listen(
        store: &mut Accessor<Self::TcpSocketData>,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<StreamReader<Resource<TcpSocket>>, ErrorCode>> {
        store.with(|mut store| {
            let data = store.data_mut();
            if !data.sockets().allowed_network_uses.tcp {
                return Ok(Err(ErrorCode::AccessDenied));
            }
            let sock = {
                let socket = get_socket_mut(data.table(), &socket)?;
                match mem::replace(&mut socket.tcp_state, TcpState::Closed) {
                    TcpState::Default(sock) | TcpState::Bound(sock) => sock,
                    tcp_state => {
                        socket.tcp_state = tcp_state;
                        return Ok(Err(ErrorCode::InvalidState));
                    }
                }
            };
            let (tx, rx) = stream(&mut store).context("failed to create stream")?;
            let socket = get_socket_mut(store.data_mut().table(), &socket)?;
            match sock.listen(socket.listen_backlog_size) {
                Ok(listener) => {
                    let listener = Arc::new(listener);
                    socket.tcp_state = TcpState::Listening {
                        task: AbortOnDropJoinHandle::from(spawn({
                            let listener = Arc::clone(&listener);
                            async move {
                                _ = tx;
                                loop {
                                    match listener.accept().await {
                                        Ok((mut stream, addr)) => {
                                            // TODO: find a way to create a socket resource
                                            eprintln!("accepted TCP connection from {addr}");
                                            if let Err(err) = stream.shutdown().await {
                                                eprintln!(
                                                    "failed to shutdown accepted stream: {err:?}"
                                                )
                                            }
                                        }
                                        Err(err) => {
                                            // TODO: refactor WIT interface to allow sending a
                                            // result
                                            eprintln!("failed to accept TCP connection: {err:?}");
                                        }
                                    }
                                }
                            }
                        })),
                        listener,
                    };
                    if true {
                        todo!("`listen`");
                    }
                    Ok(Ok(rx))
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
        })
    }

    async fn send(
        store: &mut Accessor<Self::TcpSocketData>,
        socket: Resource<TcpSocket>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let (stream, fut) = match store.with(|mut store| {
            let fut = data.read(&mut store).context("failed to get data stream")?;
            let sock = get_socket(store.data_mut().table(), &socket)?;
            if let TcpState::Connected(stream) = &sock.tcp_state {
                Ok(Ok((Arc::clone(&stream), fut)))
            } else {
                Ok(Err(ErrorCode::InvalidState))
            }
        }) {
            Ok(Ok((stream, fut))) => (stream, fut),
            Ok(Err(err)) => return Ok(Err(err)),
            Err(err) => return Err(err),
        };
        let Some((tail, buf)) = fut.into_future().await else {
            match stream
                .as_socketlike_view::<std::net::TcpStream>()
                .shutdown(Shutdown::Write)
            {
                Ok(()) => return Ok(Ok(())),
                Err(err) => return Ok(Err(err.into())),
            }
        };
        let mut fut = next_item(store, tail)?;
        // TODO: Verify that stream buffering is actually desired here, if so - limit memory usage
        let mut bufs = VecDeque::from([buf]);
        let mut eof = false;
        loop {
            match bufs.as_mut_slices() {
                ([], []) => {
                    if eof {
                        match stream
                            .as_socketlike_view::<std::net::TcpStream>()
                            .shutdown(Shutdown::Write)
                        {
                            Ok(()) => return Ok(Ok(())),
                            Err(err) => return Ok(Err(err.into())),
                        }
                    }
                }
                ([buf], []) | ([], [buf]) => match stream.try_write(&buf) {
                    Ok(n) => {
                        if n == buf.len() {
                            bufs.clear();
                        } else {
                            *buf = buf.split_off(n);
                        }
                        continue;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(err) => return Ok(Err(err.into())),
                },
                _ => match stream.try_write_vectored(
                    &bufs
                        .iter()
                        .map(|buf| IoSlice::new(buf))
                        .collect::<Box<[_]>>(),
                ) {
                    Ok(mut n) => {
                        let mut i: usize = 0;
                        for buf in bufs.iter_mut() {
                            let len = buf.len();
                            if n < len {
                                *buf = buf.split_off(n);
                                break;
                            } else if n == len {
                                i = i.saturating_add(1);
                                break;
                            }
                            n = n.saturating_sub(len);
                            i = i.saturating_add(1);
                        }
                        if i == bufs.len() {
                            bufs.clear();
                        } else if i > 0 {
                            bufs.rotate_left(i);
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(err) => return Ok(Err(err.into())),
                },
            }
            match poll_fn(|cx| {
                if !eof {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(Some((tail, chunk))) => match next_item(store, tail) {
                            Ok(next) => {
                                fut = next;
                                bufs.push_front(chunk);
                                return Poll::Ready(Ok(Ok(())));
                            }
                            Err(err) => return Poll::Ready(Err(err)),
                        },
                        Poll::Ready(None) => {
                            eof = true;
                            return Poll::Ready(Ok(Ok(())));
                        }
                        Poll::Pending => {}
                    }
                }
                match pin!(stream.writable()).poll(cx) {
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(Ok(()))),
                    Poll::Ready(Err(err)) => Poll::Ready(Ok(Err(err))),
                    Poll::Pending => Poll::Pending,
                }
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(err)) => return Ok(Err(err.into())),
                Err(err) => return Err(err),
            }
        }
    }

    fn receive(
        &mut self,
        _socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<Result<(), ErrorCode>>)> {
        todo!()
    }

    fn local_address(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.local_address())
    }

    fn remote_address(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<IpSocketAddress, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.remote_address())
    }

    fn is_listening(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<bool> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.is_listening())
    }

    fn address_family(&mut self, socket: Resource<TcpSocket>) -> wasmtime::Result<IpAddressFamily> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.address_family())
    }

    fn set_listen_backlog_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket_mut(self.table(), &socket)?;
        Ok(sock.set_listen_backlog_size(value))
    }

    fn keep_alive_enabled(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<bool, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.keep_alive_enabled())
    }

    fn set_keep_alive_enabled(
        &mut self,
        socket: Resource<TcpSocket>,
        value: bool,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.set_keep_alive_enabled(value))
    }

    fn keep_alive_idle_time(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<Duration, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.keep_alive_idle_time())
    }

    fn set_keep_alive_idle_time(
        &mut self,
        socket: Resource<TcpSocket>,
        value: Duration,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket_mut(self.table(), &socket)?;
        Ok(sock.set_keep_alive_idle_time(value))
    }

    fn keep_alive_interval(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<Duration, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.keep_alive_interval())
    }

    fn set_keep_alive_interval(
        &mut self,
        socket: Resource<TcpSocket>,
        value: Duration,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.set_keep_alive_interval(value))
    }

    fn keep_alive_count(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u32, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.keep_alive_count())
    }

    fn set_keep_alive_count(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u32,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.set_keep_alive_count(value))
    }

    fn hop_limit(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u8, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.hop_limit())
    }

    fn set_hop_limit(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u8,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.set_hop_limit(value))
    }

    fn receive_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.receive_buffer_size())
    }

    fn set_receive_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket_mut(self.table(), &socket)?;
        Ok(sock.set_receive_buffer_size(value))
    }

    fn send_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<u64, ErrorCode>> {
        let sock = get_socket(self.table(), &socket)?;
        Ok(sock.send_buffer_size())
    }

    fn set_send_buffer_size(
        &mut self,
        socket: Resource<TcpSocket>,
        value: u64,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let sock = get_socket_mut(self.table(), &socket)?;
        Ok(sock.set_send_buffer_size(value))
    }

    fn drop(&mut self, rep: Resource<TcpSocket>) -> wasmtime::Result<()> {
        self.table()
            .delete(rep)
            .context("failed to delete socket resource from table")?;
        Ok(())
    }
}
