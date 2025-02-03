use core::fmt::Debug;
use core::net::SocketAddr;

use std::os::fd::{AsFd as _, BorrowedFd};
use std::sync::Arc;

use cap_net_ext::AddressFamily;
use rustix::io::Errno;
use rustix::net::sockopt;
use tokio::sync::oneshot;

use crate::p3::bindings::sockets::types::{Duration, ErrorCode, IpAddressFamily, IpSocketAddress};
use crate::p3::sockets::SocketAddressFamily;
use crate::runtime::with_ambient_tokio_runtime;

use super::util::{normalize_get_buffer_size, normalize_set_buffer_size};

/// Value taken from rust std library.
const DEFAULT_BACKLOG: u32 = 128;

/// The state of a TCP socket.
///
/// This represents the various states a socket can be in during the
/// activities of binding, listening, accepting, and connecting.
pub enum TcpState {
    /// The initial state for a newly-created socket.
    Default(tokio::net::TcpSocket),

    /// Binding finished. The socket has an address but is not yet listening for connections.
    Bound(tokio::net::TcpSocket),

    /// The socket is now listening and waiting for an incoming connection.
    Listening {
        listener: Arc<tokio::net::TcpListener>,
        abort: oneshot::Sender<()>,
    },

    /// An outgoing connection is started.
    Connecting,

    /// A connection has been established.
    Connected {
        stream: Arc<tokio::net::TcpStream>,
        abort_receive: Option<oneshot::Sender<()>>,
    },

    Error(ErrorCode),

    Closed,
}

impl Debug for TcpState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default(_) => f.debug_tuple("Default").finish(),
            Self::Bound(_) => f.debug_tuple("Bound").finish(),
            Self::Listening { .. } => f.debug_tuple("Listening").finish(),
            Self::Connecting => f.debug_tuple("Connecting").finish(),
            Self::Connected { .. } => f.debug_tuple("Connected").finish(),
            Self::Error(..) => f.debug_tuple("Error").finish(),
            Self::Closed => write!(f, "Closed"),
        }
    }
}

/// A host TCP socket, plus associated bookkeeping.
pub struct TcpSocket {
    /// The current state in the bind/listen/accept/connect progression.
    pub tcp_state: TcpState,

    /// The desired listen queue size.
    pub listen_backlog_size: u32,

    pub family: SocketAddressFamily,

    // The socket options below are not automatically inherited from the listener
    // on all platforms. So we keep track of which options have been explicitly
    // set and manually apply those values to newly accepted clients.
    #[cfg(target_os = "macos")]
    pub receive_buffer_size: Arc<core::sync::atomic::AtomicUsize>,
    #[cfg(target_os = "macos")]
    pub send_buffer_size: Arc<core::sync::atomic::AtomicUsize>,
    #[cfg(target_os = "macos")]
    pub hop_limit: Arc<core::sync::atomic::AtomicU8>,
    #[cfg(target_os = "macos")]
    pub keep_alive_idle_time: Arc<core::sync::atomic::AtomicU64>, // nanoseconds
}

impl TcpSocket {
    /// Create a new socket in the given family.
    pub fn new(family: AddressFamily) -> std::io::Result<Self> {
        with_ambient_tokio_runtime(|| {
            let (socket, family) = match family {
                AddressFamily::Ipv4 => {
                    let socket = tokio::net::TcpSocket::new_v4()?;
                    (socket, SocketAddressFamily::Ipv4)
                }
                AddressFamily::Ipv6 => {
                    let socket = tokio::net::TcpSocket::new_v6()?;
                    sockopt::set_ipv6_v6only(&socket, true)?;
                    (socket, SocketAddressFamily::Ipv6)
                }
            };

            Ok(Self::from_state(TcpState::Default(socket), family))
        })
    }

    /// Create a `TcpSocket` from an existing socket.
    pub fn from_state(state: TcpState, family: SocketAddressFamily) -> Self {
        Self {
            tcp_state: state,
            listen_backlog_size: DEFAULT_BACKLOG,
            family,
            #[cfg(target_os = "macos")]
            receive_buffer_size: Arc::default(),
            #[cfg(target_os = "macos")]
            send_buffer_size: Arc::default(),
            #[cfg(target_os = "macos")]
            hop_limit: Arc::default(),
            #[cfg(target_os = "macos")]
            keep_alive_idle_time: Arc::default(),
        }
    }

    pub fn as_fd(&self) -> Result<BorrowedFd<'_>, ErrorCode> {
        match &self.tcp_state {
            TcpState::Default(socket) | TcpState::Bound(socket) => Ok(socket.as_fd()),
            TcpState::Connected { stream, .. } => Ok(stream.as_fd()),
            TcpState::Listening { listener, .. } => Ok(listener.as_fd()),
            TcpState::Connecting | TcpState::Closed => Err(ErrorCode::InvalidState),
            TcpState::Error(err) => Err(*err),
        }
    }

    pub fn local_address(&self) -> Result<IpSocketAddress, ErrorCode> {
        match &self.tcp_state {
            TcpState::Bound(socket) => {
                let addr = socket.local_addr()?;
                Ok(addr.into())
            }
            TcpState::Connected { stream, .. } => {
                let addr = stream.local_addr()?;
                Ok(addr.into())
            }
            TcpState::Listening { listener, .. } => {
                let addr = listener.local_addr()?;
                Ok(addr.into())
            }
            TcpState::Error(err) => Err(*err),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub fn remote_address(&self) -> Result<IpSocketAddress, ErrorCode> {
        match &self.tcp_state {
            TcpState::Connected { stream, .. } => {
                let addr = stream.peer_addr()?;
                Ok(addr.into())
            }
            TcpState::Error(err) => Err(*err),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub fn is_listening(&self) -> bool {
        matches!(self.tcp_state, TcpState::Listening { .. })
    }

    pub fn address_family(&self) -> IpAddressFamily {
        match self.family {
            SocketAddressFamily::Ipv4 => IpAddressFamily::Ipv4,
            SocketAddressFamily::Ipv6 => IpAddressFamily::Ipv6,
        }
    }

    pub fn set_listen_backlog_size(&mut self, value: u64) -> Result<(), ErrorCode> {
        const MIN_BACKLOG: u32 = 1;
        const MAX_BACKLOG: u32 = i32::MAX as u32; // OS'es will most likely limit it down even further.

        if value == 0 {
            return Err(ErrorCode::InvalidArgument);
        }
        // Silently clamp backlog size. This is OK for us to do, because operating systems do this too.
        let value = value
            .try_into()
            .unwrap_or(MAX_BACKLOG)
            .clamp(MIN_BACKLOG, MAX_BACKLOG);
        match &self.tcp_state {
            TcpState::Default(..) | TcpState::Bound(..) => {
                // Socket not listening yet. Stash value for first invocation to `listen`.
                self.listen_backlog_size = value;
                Ok(())
            }
            TcpState::Listening { listener, .. } => {
                // Try to update the backlog by calling `listen` again.
                // Not all platforms support this. We'll only update our own value if the OS supports changing the backlog size after the fact.
                if rustix::net::listen(&listener, value.try_into().unwrap_or(i32::MAX)).is_err() {
                    return Err(ErrorCode::NotSupported);
                }
                self.listen_backlog_size = value;
                Ok(())
            }
            TcpState::Error(err) => Err(*err),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub fn keep_alive_enabled(&self) -> Result<bool, ErrorCode> {
        let fd = self.as_fd()?;
        let v = sockopt::get_socket_keepalive(fd)?;
        Ok(v)
    }

    pub fn set_keep_alive_enabled(&self, value: bool) -> Result<(), ErrorCode> {
        let fd = self.as_fd()?;
        sockopt::set_socket_keepalive(fd, value)?;
        Ok(())
    }

    pub fn keep_alive_idle_time(&self) -> Result<Duration, ErrorCode> {
        let fd = self.as_fd()?;
        let v = sockopt::get_tcp_keepidle(fd)?;
        Ok(v.as_nanos().try_into().unwrap_or(u64::MAX))
    }

    pub fn set_keep_alive_idle_time(&mut self, value: Duration) -> Result<(), ErrorCode> {
        const NANOS_PER_SEC: u64 = 1_000_000_000;

        // Ensure that the value passed to the actual syscall never gets rounded down to 0.
        const MIN: u64 = NANOS_PER_SEC;

        // Cap it at Linux' maximum, which appears to have the lowest limit across our supported platforms.
        const MAX: u64 = (i16::MAX as u64) * NANOS_PER_SEC;

        let fd = self.as_fd()?;
        if value == 0 {
            // WIT: "If the provided value is 0, an `invalid-argument` error is returned."
            return Err(ErrorCode::InvalidArgument);
        }
        let value = value.clamp(MIN, MAX);
        sockopt::set_tcp_keepidle(fd, core::time::Duration::from_nanos(value))?;
        #[cfg(target_os = "macos")]
        {
            self.keep_alive_idle_time
                .store(value, core::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn keep_alive_interval(&self) -> Result<Duration, ErrorCode> {
        let fd = self.as_fd()?;
        let v = sockopt::get_tcp_keepintvl(fd)?;
        Ok(v.as_nanos().try_into().unwrap_or(u64::MAX))
    }

    pub fn set_keep_alive_interval(&self, value: Duration) -> Result<(), ErrorCode> {
        // Ensure that any fractional value passed to the actual syscall never gets rounded down to 0.
        const MIN_SECS: core::time::Duration = core::time::Duration::from_secs(1);

        // Cap it at Linux' maximum, which appears to have the lowest limit across our supported platforms.
        const MAX_SECS: core::time::Duration = core::time::Duration::from_secs(i16::MAX as u64);

        let fd = self.as_fd()?;
        if value == 0 {
            // WIT: "If the provided value is 0, an `invalid-argument` error is returned."
            return Err(ErrorCode::InvalidArgument);
        }
        sockopt::set_tcp_keepintvl(
            fd,
            core::time::Duration::from_nanos(value).clamp(MIN_SECS, MAX_SECS),
        )?;
        Ok(())
    }

    pub fn keep_alive_count(&self) -> Result<u32, ErrorCode> {
        let fd = self.as_fd()?;
        let v = sockopt::get_tcp_keepcnt(fd)?;
        Ok(v)
    }

    pub fn set_keep_alive_count(&self, value: u32) -> Result<(), ErrorCode> {
        const MIN_CNT: u32 = 1;
        // Cap it at Linux' maximum, which appears to have the lowest limit across our supported platforms.
        const MAX_CNT: u32 = i8::MAX as u32;

        let fd = self.as_fd()?;
        if value == 0 {
            // WIT: "If the provided value is 0, an `invalid-argument` error is returned."
            return Err(ErrorCode::InvalidArgument);
        }
        sockopt::set_tcp_keepcnt(fd, value.clamp(MIN_CNT, MAX_CNT))?;
        Ok(())
    }

    pub fn hop_limit(&self) -> Result<u8, ErrorCode> {
        let fd = self.as_fd()?;
        match self.family {
            SocketAddressFamily::Ipv4 => {
                let v = sockopt::get_ip_ttl(fd)?;
                let Ok(v) = v.try_into() else {
                    return Err(ErrorCode::NotSupported);
                };
                Ok(v)
            }
            SocketAddressFamily::Ipv6 => {
                let v = sockopt::get_ipv6_unicast_hops(fd)?;
                Ok(v)
            }
        }
    }

    pub fn set_hop_limit(&self, value: u8) -> Result<(), ErrorCode> {
        let fd = self.as_fd()?;
        if value == 0 {
            // WIT: "If the provided value is 0, an `invalid-argument` error is returned."
            //
            // A well-behaved IP application should never send out new packets with TTL 0.
            // We validate the value ourselves because OS'es are not consistent in this.
            // On Linux the validation is even inconsistent between their IPv4 and IPv6 implementation.
            return Err(ErrorCode::InvalidArgument);
        }
        match self.family {
            SocketAddressFamily::Ipv4 => {
                sockopt::set_ip_ttl(fd, value.into())?;
            }
            SocketAddressFamily::Ipv6 => {
                sockopt::set_ipv6_unicast_hops(fd, Some(value))?;
            }
        }
        #[cfg(target_os = "macos")]
        {
            self.hop_limit
                .store(value, core::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn receive_buffer_size(&self) -> Result<u64, ErrorCode> {
        let fd = self.as_fd()?;
        let v = sockopt::get_socket_recv_buffer_size(fd)?;
        Ok(normalize_get_buffer_size(v).try_into().unwrap_or(u64::MAX))
    }

    pub fn set_receive_buffer_size(&mut self, value: u64) -> Result<(), ErrorCode> {
        let fd = self.as_fd()?;
        if value == 0 {
            // WIT: "If the provided value is 0, an `invalid-argument` error is returned."
            return Err(ErrorCode::InvalidArgument);
        }
        let value = value.try_into().unwrap_or(usize::MAX);
        let value = normalize_set_buffer_size(value);
        match sockopt::set_socket_recv_buffer_size(fd, value) {
            Err(Errno::NOBUFS) => {}
            Err(err) => return Err(err.into()),
            _ => {}
        };
        #[cfg(target_os = "macos")]
        {
            self.receive_buffer_size
                .store(value, core::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn send_buffer_size(&self) -> Result<u64, ErrorCode> {
        let fd = self.as_fd()?;
        let v = sockopt::get_socket_send_buffer_size(fd)?;
        Ok(normalize_get_buffer_size(v).try_into().unwrap_or(u64::MAX))
    }

    pub fn set_send_buffer_size(&mut self, value: u64) -> Result<(), ErrorCode> {
        let fd = self.as_fd()?;
        if value == 0 {
            // WIT: "If the provided value is 0, an `invalid-argument` error is returned."
            return Err(ErrorCode::InvalidArgument);
        }
        let value = value.try_into().unwrap_or(usize::MAX);
        let value = normalize_set_buffer_size(value);
        match sockopt::set_socket_send_buffer_size(fd, value) {
            Err(Errno::NOBUFS) => {}
            Err(err) => return Err(err.into()),
            _ => {}
        };
        #[cfg(target_os = "macos")]
        {
            self.send_buffer_size
                .store(value, core::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }
}

pub fn bind(socket: &tokio::net::TcpSocket, local_address: SocketAddr) -> Result<(), ErrorCode> {
    // Automatically bypass the TIME_WAIT state when binding to a specific port
    // Unconditionally (re)set SO_REUSEADDR, even when the value is false.
    // This ensures we're not accidentally affected by any socket option
    // state left behind by a previous failed call to this method.
    #[cfg(not(windows))]
    if let Err(err) = socket.set_reuseaddr(local_address.port() > 0) {
        return Err(err.into());
    }

    // Perform the OS bind call.
    socket
        .bind(local_address)
        .map_err(|err| match Errno::from_io_error(&err) {
            // From https://pubs.opengroup.org/onlinepubs/9699919799/functions/bind.html:
            // > [EAFNOSUPPORT] The specified address is not a valid address for the address family of the specified socket
            //
            // The most common reasons for this error should have already
            // been handled by our own validation slightly higher up in this
            // function. This error mapping is here just in case there is
            // an edge case we didn't catch.
            Some(Errno::AFNOSUPPORT) => ErrorCode::InvalidArgument,
            // See: https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-bind#:~:text=WSAENOBUFS
            // Windows returns WSAENOBUFS when the ephemeral ports have been exhausted.
            #[cfg(windows)]
            Some(Errno::NOBUFS) => ErrorCode::AddressInUse,
            _ => err.into(),
        })
}
