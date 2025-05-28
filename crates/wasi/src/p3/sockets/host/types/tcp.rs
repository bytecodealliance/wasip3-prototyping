use core::mem;
use core::net::SocketAddr;

use std::net::Shutdown;
use std::sync::Arc;

use anyhow::{Context as _, bail, ensure};
use io_lifetimes::AsSocketlike as _;
use rustix::io::Errno;
use tokio::sync::mpsc;
use wasmtime::component::{
    Accessor, AccessorTask, HostFuture, HostStream, Resource, ResourceTable, StreamWriter,
};

use crate::p3::bindings::sockets::types::{
    Duration, ErrorCode, HostTcpSocket, HostTcpSocketConcurrent, IpAddressFamily, IpSocketAddress,
    TcpSocket,
};
use crate::p3::sockets::tcp::TcpState;
use crate::p3::sockets::util::{
    is_valid_address_family, is_valid_remote_address, is_valid_unicast_address,
};
use crate::p3::sockets::{
    SocketAddrUse, SocketAddressFamily, WasiSockets, WasiSocketsImpl, WasiSocketsView,
};
use crate::p3::{AccessorTaskFn, IoTask, ResourceView as _};

use super::is_addr_allowed;

fn is_tcp_allowed<T, U>(store: &mut Accessor<T, WasiSockets<U>>) -> bool
where
    U: WasiSocketsView + 'static,
{
    store.with(|mut view| view.get().sockets().allowed_network_uses.tcp)
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

struct ListenTask {
    family: SocketAddressFamily,
    tx: StreamWriter<Option<Resource<TcpSocket>>>,
    rx: mpsc::Receiver<std::io::Result<(tokio::net::TcpStream, SocketAddr)>>,

    // The socket options below are not automatically inherited from the listener
    // on all platforms. So we keep track of which options have been explicitly
    // set and manually apply those values to newly accepted clients.
    #[cfg(target_os = "macos")]
    receive_buffer_size: Arc<core::sync::atomic::AtomicUsize>,
    #[cfg(target_os = "macos")]
    send_buffer_size: Arc<core::sync::atomic::AtomicUsize>,
    #[cfg(target_os = "macos")]
    hop_limit: Arc<core::sync::atomic::AtomicU8>,
    #[cfg(target_os = "macos")]
    keep_alive_idle_time: Arc<core::sync::atomic::AtomicU64>, // nanoseconds
}

impl<T, U> AccessorTask<T, WasiSockets<U>, wasmtime::Result<()>> for ListenTask
where
    U: WasiSocketsView + 'static,
{
    async fn run(mut self, store: &mut Accessor<T, WasiSockets<U>>) -> wasmtime::Result<()> {
        let mut tx = self.tx;
        while let Some(res) = self.rx.recv().await {
            let state = match res {
                Ok((stream, _addr)) => {
                    #[cfg(target_os = "macos")]
                    {
                        // Manually inherit socket options from listener. We only have to
                        // do this on platforms that don't already do this automatically
                        // and only if a specific value was explicitly set on the listener.

                        let receive_buffer_size = self
                            .receive_buffer_size
                            .load(core::sync::atomic::Ordering::Relaxed);
                        if receive_buffer_size > 0 {
                            // Ignore potential error.
                            _ = rustix::net::sockopt::set_socket_recv_buffer_size(
                                &stream,
                                receive_buffer_size,
                            );
                        }

                        let send_buffer_size = self
                            .send_buffer_size
                            .load(core::sync::atomic::Ordering::Relaxed);
                        if send_buffer_size > 0 {
                            // Ignore potential error.
                            _ = rustix::net::sockopt::set_socket_send_buffer_size(
                                &stream,
                                send_buffer_size,
                            );
                        }

                        // For some reason, IP_TTL is inherited, but IPV6_UNICAST_HOPS isn't.
                        if self.family == SocketAddressFamily::Ipv6 {
                            let hop_limit =
                                self.hop_limit.load(core::sync::atomic::Ordering::Relaxed);
                            if hop_limit > 0 {
                                // Ignore potential error.
                                _ = rustix::net::sockopt::set_ipv6_unicast_hops(
                                    &stream,
                                    Some(hop_limit),
                                );
                            }
                        }

                        let keep_alive_idle_time = self
                            .keep_alive_idle_time
                            .load(core::sync::atomic::Ordering::Relaxed);
                        if keep_alive_idle_time > 0 {
                            // Ignore potential error.
                            _ = rustix::net::sockopt::set_tcp_keepidle(
                                &stream,
                                core::time::Duration::from_nanos(keep_alive_idle_time),
                            );
                        }
                    }
                    TcpState::Connected {
                        stream: Arc::new(stream),
                        rx_task: None,
                    }
                }
                Err(err) => {
                    match Errno::from_io_error(&err) {
                        // From: https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept#:~:text=WSAEINPROGRESS
                        // > WSAEINPROGRESS: A blocking Windows Sockets 1.1 call is in progress,
                        // > or the service provider is still processing a callback function.
                        //
                        // wasi-sockets doesn't have an equivalent to the EINPROGRESS error,
                        // because in POSIX this error is only returned by a non-blocking
                        // `connect` and wasi-sockets has a different solution for that.
                        #[cfg(windows)]
                        Some(Errno::INPROGRESS) => TcpState::Error(ErrorCode::Unknown),

                        // Normalize Linux' non-standard behavior.
                        //
                        // From https://man7.org/linux/man-pages/man2/accept.2.html:
                        // > Linux accept() passes already-pending network errors on the
                        // > new socket as an error code from accept(). This behavior
                        // > differs from other BSD socket implementations. (...)
                        #[cfg(target_os = "linux")]
                        Some(
                            Errno::CONNRESET
                            | Errno::NETRESET
                            | Errno::HOSTUNREACH
                            | Errno::HOSTDOWN
                            | Errno::NETDOWN
                            | Errno::NETUNREACH
                            | Errno::PROTO
                            | Errno::NOPROTOOPT
                            | Errno::NONET
                            | Errno::OPNOTSUPP,
                        ) => TcpState::Error(ErrorCode::ConnectionAborted),
                        _ => TcpState::Error(err.into()),
                    }
                }
            };
            let fut = store.with(|mut view| {
                let socket = view
                    .get()
                    .table()
                    .push(TcpSocket::from_state(state, self.family))
                    .context("failed to push socket to table")?;
                Ok::<_, wasmtime::Error>(tx.write(Some(socket)))
            })?;
            let (Some(tail), _) = fut.await else {
                return Ok(());
            };
            tx = tail;
        }
        Ok(())
    }
}

impl<T> HostTcpSocketConcurrent for WasiSockets<T>
where
    T: WasiSocketsView + 'static,
{
    async fn bind<U>(
        store: &mut Accessor<U, Self>,
        socket: Resource<TcpSocket>,
        local_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let local_address = SocketAddr::from(local_address);
        if !is_tcp_allowed(store)
            || !is_addr_allowed(store, local_address, SocketAddrUse::TcpBind).await
        {
            return Ok(Err(ErrorCode::AccessDenied));
        }
        store.with(|mut view| {
            let mut binding = view.get();
            let socket = get_socket_mut(binding.table(), &socket)?;
            Ok(socket.bind(local_address))
        })
    }

    async fn connect<U>(
        store: &mut Accessor<U, Self>,
        socket: Resource<TcpSocket>,
        remote_address: IpSocketAddress,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let remote_address = SocketAddr::from(remote_address);
        if !is_tcp_allowed(store)
            || !is_addr_allowed(store, remote_address, SocketAddrUse::TcpConnect).await
        {
            return Ok(Err(ErrorCode::AccessDenied));
        }
        match store.with(|mut view| {
            let ip = remote_address.ip();
            let mut binding = view.get();
            let socket = get_socket_mut(binding.table(), &socket)?;
            if !is_valid_unicast_address(ip)
                || !is_valid_remote_address(remote_address)
                || !is_valid_address_family(ip, socket.family)
            {
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
                store.with(|mut view| {
                    let mut binding = view.get();
                    let socket = get_socket_mut(binding.table(), &socket)?;
                    ensure!(
                        matches!(socket.tcp_state, TcpState::Connecting),
                        "corrupted socket state"
                    );
                    match res {
                        Ok(stream) => {
                            socket.tcp_state = TcpState::Connected {
                                stream: Arc::new(stream),
                                rx_task: None,
                            };
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

    async fn listen<U: 'static>(
        store: &mut Accessor<U, Self>,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<Result<HostStream<Resource<TcpSocket>>, ErrorCode>> {
        match store.with(|mut view| {
            if !view.get().sockets().allowed_network_uses.tcp {
                return Ok(Err(ErrorCode::AccessDenied));
            }
            let sock = {
                let mut binding = view.get();
                let socket = get_socket_mut(binding.table(), &socket)?;
                match mem::replace(&mut socket.tcp_state, TcpState::Closed) {
                    TcpState::Default(sock) | TcpState::Bound(sock) => sock,
                    tcp_state => {
                        socket.tcp_state = tcp_state;
                        return Ok(Err(ErrorCode::InvalidState));
                    }
                }
            };
            let instance = view.instance();
            let (tx, rx) = instance
                .stream::<_, _, Vec<_>, _, _>(&mut view)
                .context("failed to create stream")?;
            let &TcpSocket {
                listen_backlog_size,
                ..
            } = get_socket(view.get().table(), &socket)?;

            match sock.listen(listen_backlog_size) {
                Ok(listener) => {
                    let listener = Arc::new(listener);
                    let (task_tx, task_rx) = mpsc::channel(1);
                    let task = view.spawn(AccessorTaskFn({
                        let listener = Arc::clone(&listener);
                        |_: &mut Accessor<U, Self>| async move {
                            while let Ok(tx) = task_tx.reserve().await {
                                tx.send(listener.accept().await)
                            }
                            Ok(())
                        }
                    }));
                    let mut binding = view.get();
                    let TcpSocket {
                        tcp_state,
                        family,
                        #[cfg(target_os = "macos")]
                        receive_buffer_size,
                        #[cfg(target_os = "macos")]
                        send_buffer_size,
                        #[cfg(target_os = "macos")]
                        hop_limit,
                        #[cfg(target_os = "macos")]
                        keep_alive_idle_time,
                        ..
                    } = get_socket_mut(binding.table(), &socket)?;
                    *tcp_state = TcpState::Listening {
                        listener,
                        task: task.abort_on_drop_handle(),
                    };
                    Ok(Ok((
                        rx,
                        ListenTask {
                            family: *family,
                            tx,
                            rx: task_rx,
                            #[cfg(target_os = "macos")]
                            receive_buffer_size: Arc::clone(&receive_buffer_size),
                            #[cfg(target_os = "macos")]
                            send_buffer_size: Arc::clone(&send_buffer_size),
                            #[cfg(target_os = "macos")]
                            hop_limit: Arc::clone(&hop_limit),
                            #[cfg(target_os = "macos")]
                            keep_alive_idle_time: Arc::clone(&keep_alive_idle_time),
                        },
                    )))
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
        }) {
            Ok(Ok((rx, task))) => {
                store.spawn(task);
                Ok(Ok(rx.into()))
            }
            Ok(Err(err)) => Ok(Err(err)),
            Err(err) => Err(err),
        }
    }

    async fn send<U: 'static>(
        store: &mut Accessor<U, Self>,
        socket: Resource<TcpSocket>,
        data: HostStream<u8>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let mut buf = Vec::with_capacity(8096);
        let (stream, fut) = match store.with(|mut view| {
            let data = data.into_reader::<Vec<u8>, _, _>(&mut view);
            let fut = data.read(buf);
            let mut binding = view.get();
            let sock = get_socket(binding.table(), &socket)?;
            if let TcpState::Connected { stream, .. } = &sock.tcp_state {
                Ok(Ok((Arc::clone(&stream), fut)))
            } else {
                Ok(Err(ErrorCode::InvalidState))
            }
        }) {
            Ok(Ok((stream, fut))) => (stream, fut),
            Ok(Err(err)) => return Ok(Err(err)),
            Err(err) => return Err(err),
        };
        let mut fut = fut;
        'outer: loop {
            let (Some(tail), buf_again) = fut.await else {
                _ = stream
                    .as_socketlike_view::<std::net::TcpStream>()
                    .shutdown(Shutdown::Write);
                return Ok(Ok(()));
            };
            let mut slice = buf_again.as_slice();
            loop {
                match stream.try_write(&slice) {
                    Ok(n) => {
                        if n == slice.len() {
                            buf = buf_again;
                            buf.clear();
                            fut = tail.read(buf);
                            continue 'outer;
                        } else {
                            slice = &slice[n..];
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if let Err(err) = stream.writable().await {
                            _ = stream
                                .as_socketlike_view::<std::net::TcpStream>()
                                .shutdown(Shutdown::Write);
                            return Ok(Err(err.into()));
                        }
                    }
                    Err(err) => {
                        _ = stream
                            .as_socketlike_view::<std::net::TcpStream>()
                            .shutdown(Shutdown::Write);
                        return Ok(Err(err.into()));
                    }
                }
            }
        }
    }

    async fn receive<U: 'static>(
        store: &mut Accessor<U, Self>,
        socket: Resource<TcpSocket>,
    ) -> wasmtime::Result<(HostStream<u8>, HostFuture<Result<(), ErrorCode>>)> {
        store.with(|mut view| {
            let instance = view.instance();
            let (data_tx, data_rx) = instance
                .stream::<_, _, Vec<_>, _, _>(&mut view)
                .context("failed to create stream")?;
            let (res_tx, res_rx) = instance
                .future(|| unreachable!(), &mut view)
                .context("failed to create future")?;
            let mut binding = view.get();
            let sock = get_socket(binding.table(), &socket)?;
            match &sock.tcp_state {
                TcpState::Connected {
                    stream,
                    rx_task: None,
                } => {
                    let (task_tx, task_rx) = mpsc::channel(1);
                    let stream = Arc::clone(&stream);
                    let task = view.spawn(AccessorTaskFn({
                        |_: &mut Accessor<U, Self>| async move {
                            while let Ok(tx) = task_tx.reserve().await {
                                let mut buf = vec![0; 8096];
                                match stream.try_read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        buf.truncate(n);
                                        tx.send(Ok(buf));
                                    }
                                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                        if let Err(err) = stream.readable().await {
                                            tx.send(Err(err.into()));
                                            break;
                                        }
                                    }
                                    Err(err) => {
                                        tx.send(Err(err.into()));
                                        break;
                                    }
                                }
                            }
                            _ = stream
                                .as_socketlike_view::<std::net::TcpStream>()
                                .shutdown(Shutdown::Read);
                            Ok(())
                        }
                    }));
                    view.spawn(IoTask {
                        data: data_tx,
                        result: res_tx,
                        rx: task_rx,
                    });
                    let mut binding = view.get();
                    let TcpSocket {
                        tcp_state: TcpState::Connected { rx_task, .. },
                        ..
                    } = get_socket_mut(binding.table(), &socket)?
                    else {
                        bail!("corrupted socket state");
                    };
                    *rx_task = Some(task.abort_on_drop_handle());
                }
                _ => {
                    let fut = res_tx.write(Err(ErrorCode::InvalidState));
                    view.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                        fut.await;
                        Ok(())
                    }));
                }
            }
            Ok((data_rx.into(), res_rx.into()))
        })
    }
}

impl<T> HostTcpSocket for WasiSocketsImpl<T>
where
    T: WasiSocketsView,
{
    fn new(&mut self, address_family: IpAddressFamily) -> wasmtime::Result<Resource<TcpSocket>> {
        let socket = TcpSocket::new(address_family.into()).context("failed to create socket")?;
        self.table()
            .push(socket)
            .context("failed to push socket resource to table")
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
