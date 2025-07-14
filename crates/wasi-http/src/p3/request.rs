use core::time::Duration;

use std::sync::Arc;

use http::uri::{Authority, PathAndQuery, Scheme};
use http::{HeaderMap, Method};
use wasmtime_wasi::p3::{AbortOnDropHandle, WithChildren};

use crate::p3::Body;

/// The concrete type behind a `wasi:http/types/request-options` resource.
#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
    /// How long to wait for a connection to be established.
    pub connect_timeout: Option<Duration>,
    /// How long to wait for the first byte of the response body.
    pub first_byte_timeout: Option<Duration>,
    /// How long to wait between frames of the response body.
    pub between_bytes_timeout: Option<Duration>,
}
/// The concrete type behind a `wasi:http/types/request` resource.
pub struct Request {
    /// The method of the request.
    pub method: Method,
    /// The scheme of the request.
    pub scheme: Option<Scheme>,
    /// The authority of the request.
    pub authority: Option<Authority>,
    /// The path and query of the request.
    pub path_with_query: Option<PathAndQuery>,
    /// The request headers.
    pub headers: WithChildren<HeaderMap>,
    /// Request options.
    pub options: Option<WithChildren<RequestOptions>>,
    /// The request body.
    pub(crate) body: Arc<std::sync::Mutex<Body>>,
    /// Body stream task handle
    pub(crate) task: Option<AbortOnDropHandle>,
}

impl Request {
    /// Construct a new [Request]
    pub fn new(
        method: Method,
        scheme: Option<Scheme>,
        authority: Option<Authority>,
        path_with_query: Option<PathAndQuery>,
        headers: HeaderMap,
        body: impl Into<Body>,
        options: Option<RequestOptions>,
    ) -> Self {
        Self {
            method,
            scheme,
            authority,
            path_with_query,
            headers: WithChildren::new(headers),
            body: Arc::new(std::sync::Mutex::new(body.into())),
            options: options.map(WithChildren::new),
            task: None,
        }
    }
}
