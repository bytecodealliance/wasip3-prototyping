use core::future::Future;
use core::time::Duration;

use bytes::Bytes;
use http::uri::Scheme;
use http_body_util::BodyExt as _;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::io::TokioIo;
use crate::p3::bindings::http::types::{DnsErrorPayload, ErrorCode};
use crate::p3::{IncomingResponseBody, RequestOptions};

fn dns_error(rcode: String, info_code: u16) -> ErrorCode {
    ErrorCode::DnsError(DnsErrorPayload {
        rcode: Some(rcode),
        info_code: Some(info_code),
    })
}

/// HTTP client
pub trait Client: Clone + Send + Sync {
    /// Error returned by `send_request`
    type Error: Into<ErrorCode> + Send;

    /// Whether to set `host` header in the request passed to `send_request`.
    fn set_host_header(&mut self) -> bool {
        true
    }

    /// Scheme to default to, when not set by the guest.
    ///
    /// If [None], `handle` will return [ErrorCode::HttpProtocolError]
    /// for requests missing a scheme.
    fn default_scheme(&mut self) -> Option<http::uri::Scheme> {
        Some(Scheme::HTTPS)
    }

    /// Whether a given scheme should be considered supported.
    ///
    /// `handle` will return [ErrorCode::HttpProtocolError] for unsupported schemes.
    fn is_supported_scheme(&mut self, scheme: &http::uri::Scheme) -> bool {
        *scheme == Scheme::HTTP || *scheme == Scheme::HTTPS
    }

    /// Send an outgoing request.
    fn send_request(
        &mut self,
        request: http::Request<
            impl http_body::Body<Data = Bytes, Error = Option<ErrorCode>> + Send + 'static,
        >,
        options: Option<RequestOptions>,
    ) -> impl Future<
        Output = wasmtime::Result<
            Result<
                (
                    impl Future<
                        Output = Result<
                            http::Response<
                                impl http_body::Body<Data = Bytes, Error = Self::Error>
                                + Send
                                + Sync
                                + 'static,
                            >,
                            ErrorCode,
                        >,
                    > + Send,
                    impl Future<Output = Result<(), Self::Error>> + Send + 'static,
                ),
                ErrorCode,
            >,
        >,
    > + Send;
}

/// Default HTTP client
#[derive(Clone, Debug, Default)]
pub struct DefaultClient;

impl Client for DefaultClient {
    type Error = ErrorCode;

    async fn send_request(
        &mut self,
        request: http::Request<
            impl http_body::Body<Data = Bytes, Error = Option<ErrorCode>> + Send + 'static,
        >,
        options: Option<RequestOptions>,
    ) -> wasmtime::Result<
        Result<
            (
                impl Future<
                    Output = Result<
                        http::Response<
                            impl http_body::Body<Data = Bytes, Error = Self::Error> + 'static,
                        >,
                        ErrorCode,
                    >,
                >,
                impl Future<Output = Result<(), Self::Error>> + 'static,
            ),
            ErrorCode,
        >,
    > {
        Ok(default_send_request(request, options).await)
    }
}

/// The default implementation of how an outgoing request is sent.
///
/// This implementation is used by the `wasi:http/outgoing-handler` interface
/// default implementation.
pub async fn default_send_request(
    mut request: http::Request<
        impl http_body::Body<Data = Bytes, Error = Option<ErrorCode>> + Send + 'static,
    >,
    options: Option<RequestOptions>,
) -> Result<
    (
        impl Future<
            Output = Result<
                http::Response<impl http_body::Body<Data = Bytes, Error = ErrorCode>>,
                ErrorCode,
            >,
        >,
        impl Future<Output = Result<(), ErrorCode>>,
    ),
    ErrorCode,
> {
    trait TokioStream: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static {
        fn boxed(self) -> Box<dyn TokioStream>
        where
            Self: Sized,
        {
            Box::new(self)
        }
    }
    impl<T> TokioStream for T where T: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static {}

    let uri = request.uri();
    let authority = uri.authority().ok_or(ErrorCode::HttpRequestUriInvalid)?;
    let use_tls = uri.scheme() == Some(&Scheme::HTTPS);
    let authority = if authority.port().is_some() {
        authority.to_string()
    } else {
        let port = if use_tls { 443 } else { 80 };
        format!("{authority}:{port}")
    };

    let connect_timeout = options
        .as_ref()
        .and_then(
            |RequestOptions {
                 connect_timeout, ..
             }| *connect_timeout,
        )
        .unwrap_or(Duration::from_secs(600));

    let first_byte_timeout = options
        .as_ref()
        .and_then(
            |RequestOptions {
                 first_byte_timeout, ..
             }| *first_byte_timeout,
        )
        .unwrap_or(Duration::from_secs(600));

    let between_bytes_timeout = options
        .as_ref()
        .and_then(
            |RequestOptions {
                 between_bytes_timeout,
                 ..
             }| *between_bytes_timeout,
        )
        .unwrap_or(Duration::from_secs(600));

    let stream = match tokio::time::timeout(connect_timeout, TcpStream::connect(&authority)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(err)) if err.kind() == std::io::ErrorKind::AddrNotAvailable => {
            return Err(dns_error("address not available".to_string(), 0));
        }
        Ok(Err(err))
            if err
                .to_string()
                .starts_with("failed to lookup address information") =>
        {
            return Err(dns_error("address not available".to_string(), 0));
        }
        Ok(Err(..)) => return Err(ErrorCode::ConnectionRefused),
        Err(..) => return Err(ErrorCode::ConnectionTimeout),
    };
    let stream = if use_tls {
        use rustls::pki_types::ServerName;

        // derived from https://github.com/rustls/rustls/blob/main/examples/src/bin/simpleclient.rs
        let root_cert_store = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
        };
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let mut parts = authority.split(":");
        let host = parts.next().unwrap_or(&authority);
        let domain = ServerName::try_from(host)
            .map_err(|e| {
                tracing::warn!("dns lookup error: {e:?}");
                dns_error("invalid dns name".to_string(), 0)
            })?
            .to_owned();
        let stream = connector.connect(domain, stream).await.map_err(|e| {
            tracing::warn!("tls protocol error: {e:?}");
            ErrorCode::TlsProtocolError
        })?;
        stream.boxed()
    } else {
        stream.boxed()
    };
    let (mut sender, conn) = tokio::time::timeout(
        connect_timeout,
        // TODO: we should plumb the builder through the http context, and use it here
        hyper::client::conn::http1::Builder::new().handshake(TokioIo::new(stream)),
    )
    .await
    .map_err(|_| ErrorCode::ConnectionTimeout)?
    .map_err(ErrorCode::from_hyper_request_error)?;

    // at this point, the request contains the scheme and the authority, but
    // the http packet should only include those if addressing a proxy, so
    // remove them here, since SendRequest::send_request does not do it for us
    *request.uri_mut() = http::Uri::builder()
        .path_and_query(
            request
                .uri()
                .path_and_query()
                .map(|p| p.as_str())
                .unwrap_or("/"),
        )
        .build()
        .expect("comes from valid request");

    let request =
        request.map(|body| body.map_err(|err| err.unwrap_or(ErrorCode::InternalError(None))));
    Ok((
        async move {
            let response = tokio::time::timeout(first_byte_timeout, sender.send_request(request))
                .await
                .map_err(|_| ErrorCode::ConnectionReadTimeout)?
                .map_err(ErrorCode::from_hyper_request_error)?;
            let mut timeout = tokio::time::interval(between_bytes_timeout);
            timeout.reset();
            Ok(response.map(|incoming| IncomingResponseBody { incoming, timeout }))
        },
        async move {
            conn.await.map_err(ErrorCode::from_hyper_request_error)?;
            Ok(())
        },
    ))
}
