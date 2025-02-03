use core::future::Future;
use core::net::Ipv6Addr;
use core::str::FromStr as _;

use tokio::net::lookup_host;
use wasmtime::component::for_any;
use wasmtime::StoreContextMut;

use crate::p3::bindings::sockets::ip_name_lookup::{ErrorCode, Host};
use crate::p3::bindings::sockets::types;
use crate::p3::sockets::util::{from_ipv4_addr, from_ipv6_addr};
use crate::p3::sockets::{WasiSocketsImpl, WasiSocketsView};

impl<T> Host for WasiSocketsImpl<&mut T>
where
    T: WasiSocketsView,
{
    type Data = T;

    fn resolve_addresses(
        store: StoreContextMut<'_, Self::Data>,
        name: String,
    ) -> impl Future<
        Output = impl FnOnce(
            StoreContextMut<'_, Self::Data>,
        ) -> wasmtime::Result<Result<Vec<types::IpAddress>, ErrorCode>>
                     + 'static,
    > + 'static {
        // `url::Host::parse` serves us two functions:
        // 1. validate the input is a valid domain name or IP,
        // 2. convert unicode domains to punycode.
        let mut host = if let Ok(host) = url::Host::parse(&name) {
            Ok(host)
        } else if let Ok(addr) = Ipv6Addr::from_str(&name) {
            // `url::Host::parse` doesn't understand bare IPv6 addresses without [brackets]
            Ok(url::Host::Ipv6(addr))
        } else {
            Err(ErrorCode::InvalidArgument)
        };
        if host.is_ok() && !store.data().sockets().allowed_network_uses.ip_name_lookup {
            host = Err(ErrorCode::PermanentResolverFailure);
        }
        async move {
            let res = match host {
                Ok(url::Host::Ipv4(addr)) => Ok(vec![types::IpAddress::Ipv4(from_ipv4_addr(addr))]),
                Ok(url::Host::Ipv6(addr)) => Ok(vec![types::IpAddress::Ipv6(from_ipv6_addr(addr))]),
                Ok(url::Host::Domain(domain)) => {
                    // This is only resolving names, not ports, so force the port to be 0.
                    if let Ok(addrs) = lookup_host((domain.as_str(), 0)).await {
                        Ok(addrs.map(|addr| addr.ip().to_canonical().into()).collect())
                    } else {
                        // If/when we use `getaddrinfo` directly, map the error properly.
                        Err(ErrorCode::NameUnresolvable)
                    }
                }
                Err(err) => Err(err),
            };
            for_any(move |_| Ok(res))
        }
    }
}
