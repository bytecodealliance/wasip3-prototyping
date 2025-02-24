use core::net::SocketAddr;

use cap_net_ext::{AddressFamily, Blocking, UdpSocketExt as _};
use io_lifetimes::raw::{FromRawSocketlike as _, IntoRawSocketlike as _};
use io_lifetimes::AsSocketlike as _;
use rustix::io::Errno;
use rustix::net::bind;

use crate::p3::bindings::sockets::types::{ErrorCode, IpAddressFamily, IpSocketAddress};
use crate::p3::sockets::util::{
    get_unicast_hop_limit, receive_buffer_size, send_buffer_size, set_receive_buffer_size,
    set_send_buffer_size, set_unicast_hop_limit,
};
use crate::p3::sockets::SocketAddressFamily;
use crate::runtime::with_ambient_tokio_runtime;

use super::util::is_valid_address_family;

/// The state of a UDP socket.
///
/// This represents the various states a socket can be in during the
/// activities of binding, and connecting.
pub(crate) enum UdpState {
    /// The initial state for a newly-created socket.
    Default,

    /// Binding finished via `finish_bind`. The socket has an address but
    /// is not yet listening for connections.
    Bound,

    /// The socket is "connected" to a peer address.
    #[allow(unused)] // TODO: Remove
    Connected,
}

/// A host UDP socket, plus associated bookkeeping.
///
/// The inner state is wrapped in an Arc because the same underlying socket is
/// used for implementing the stream types.
pub struct UdpSocket {
    socket: tokio::net::UdpSocket,

    /// The current state in the bind/connect progression.
    udp_state: UdpState,

    /// Socket address family.
    family: SocketAddressFamily,
}

impl UdpSocket {
    /// Create a new socket in the given family.
    pub fn new(family: AddressFamily) -> std::io::Result<Self> {
        // Delegate socket creation to cap_net_ext. They handle a couple of things for us:
        // - On Windows: call WSAStartup if not done before.
        // - Set the NONBLOCK and CLOEXEC flags. Either immediately during socket creation,
        //   or afterwards using ioctl or fcntl. Exact method depends on the platform.

        let fd = cap_std::net::UdpSocket::new(family, Blocking::No)?;

        let socket_address_family = match family {
            AddressFamily::Ipv4 => SocketAddressFamily::Ipv4,
            AddressFamily::Ipv6 => {
                rustix::net::sockopt::set_ipv6_v6only(&fd, true)?;
                SocketAddressFamily::Ipv6
            }
        };

        let socket = with_ambient_tokio_runtime(|| {
            tokio::net::UdpSocket::try_from(unsafe {
                std::net::UdpSocket::from_raw_socketlike(fd.into_raw_socketlike())
            })
        })?;

        Ok(Self {
            socket,
            udp_state: UdpState::Default,
            family: socket_address_family,
        })
    }

    pub fn bind(&mut self, addr: SocketAddr) -> Result<(), ErrorCode> {
        if !matches!(self.udp_state, UdpState::Default) {
            return Err(ErrorCode::InvalidState);
        }
        if !is_valid_address_family(addr.ip(), self.family) {
            return Err(ErrorCode::InvalidArgument);
        }
        bind(&self.socket, &addr).map_err(|err| match err {
            // See: https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-bind#:~:text=WSAENOBUFS
            // Windows returns WSAENOBUFS when the ephemeral ports have been exhausted.
            #[cfg(windows)]
            Errno::NOBUFS => ErrorCode::AddressInUse,
            Errno::AFNOSUPPORT => ErrorCode::InvalidArgument,
            _ => err.into(),
        })?;
        self.udp_state = UdpState::Bound;
        Ok(())
    }

    pub fn local_address(&self) -> Result<IpSocketAddress, ErrorCode> {
        if matches!(self.udp_state, UdpState::Default) {
            return Err(ErrorCode::InvalidState);
        }
        let addr = self
            .socket
            .as_socketlike_view::<std::net::UdpSocket>()
            .local_addr()?;
        Ok(addr.into())
    }

    pub fn remote_address(&self) -> Result<IpSocketAddress, ErrorCode> {
        if !matches!(self.udp_state, UdpState::Connected) {
            return Err(ErrorCode::InvalidState);
        }
        let addr = self
            .socket
            .as_socketlike_view::<std::net::UdpSocket>()
            .peer_addr()?;
        Ok(addr.into())
    }

    pub fn address_family(&self) -> IpAddressFamily {
        match self.family {
            SocketAddressFamily::Ipv4 => IpAddressFamily::Ipv4,
            SocketAddressFamily::Ipv6 => IpAddressFamily::Ipv6,
        }
    }

    pub fn unicast_hop_limit(&self) -> Result<u8, ErrorCode> {
        get_unicast_hop_limit(&self.socket, self.family)
    }

    pub fn set_unicast_hop_limit(&self, value: u8) -> Result<(), ErrorCode> {
        set_unicast_hop_limit(&self.socket, self.family, value)
    }

    pub fn receive_buffer_size(&self) -> Result<u64, ErrorCode> {
        receive_buffer_size(&self.socket)
    }

    pub fn set_receive_buffer_size(&self, value: u64) -> Result<(), ErrorCode> {
        set_receive_buffer_size(&self.socket, value)?;
        Ok(())
    }

    pub fn send_buffer_size(&self) -> Result<u64, ErrorCode> {
        send_buffer_size(&self.socket)
    }

    pub fn set_send_buffer_size(&self, value: u64) -> Result<(), ErrorCode> {
        set_send_buffer_size(&self.socket, value)?;
        Ok(())
    }
}
