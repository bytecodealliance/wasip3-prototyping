use core::net::Ipv6Addr;
use core::str::FromStr as _;

use tokio::net::lookup_host;
use wasmtime::component::Accessor;

use crate::p3::bindings::sockets::ip_name_lookup::{ErrorCode, Host, HostConcurrent};
use crate::p3::bindings::sockets::types;
use crate::p3::sockets::util::{from_ipv4_addr, from_ipv6_addr};
use crate::p3::sockets::{WasiSockets, WasiSocketsImpl, WasiSocketsView};

impl<T> HostConcurrent for WasiSockets<T>
where
    T: WasiSocketsView + 'static,
{
    async fn resolve_addresses<U>(
        store: &Accessor<U, Self>,
        name: String,
    ) -> wasmtime::Result<Result<Vec<types::IpAddress>, ErrorCode>> {
        // `url::Host::parse` serves us two functions:
        // 1. validate the input is a valid domain name or IP,
        // 2. convert unicode domains to punycode.
        let host = if let Ok(host) = url::Host::parse(&name) {
            host
        } else if let Ok(addr) = Ipv6Addr::from_str(&name) {
            // `url::Host::parse` doesn't understand bare IPv6 addresses without [brackets]
            url::Host::Ipv6(addr)
        } else {
            return Ok(Err(ErrorCode::InvalidArgument));
        };
        if !store.with(|mut view| view.get().sockets().allowed_network_uses.ip_name_lookup) {
            return Ok(Err(ErrorCode::PermanentResolverFailure));
        }
        match host {
            url::Host::Ipv4(addr) => Ok(Ok(vec![types::IpAddress::Ipv4(from_ipv4_addr(addr))])),
            url::Host::Ipv6(addr) => Ok(Ok(vec![types::IpAddress::Ipv6(from_ipv6_addr(addr))])),
            url::Host::Domain(domain) => {
                // This is only resolving names, not ports, so force the port to be 0.
                if let Ok(addrs) = lookup_host((domain.as_str(), 0)).await {
                    Ok(Ok(addrs
                        .map(|addr| addr.ip().to_canonical().into())
                        .collect()))
                } else {
                    // If/when we use `getaddrinfo` directly, map the error properly.
                    Ok(Err(ErrorCode::NameUnresolvable))
                }
            }
        }
    }
}

impl<T> Host for WasiSocketsImpl<T> where T: WasiSocketsView {}
