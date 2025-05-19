use core::convert::Infallible;
use core::error::Error as _;
use std::sync::Arc;

use anyhow::{Context as _, bail};
use bytes::Bytes;
use tracing::warn;

use crate::p3::bindings::http::types::{ErrorCode, Method, Scheme};
use crate::p3::{Body, Request, Response};

impl ErrorCode {
    /// Translate a [`hyper::Error`] to a wasi-http [ErrorCode] in the context of a request.
    pub fn from_hyper_request_error(err: hyper::Error) -> Self {
        // If there's a source, we might be able to extract a wasi-http error from it.
        if let Some(cause) = err.source() {
            if let Some(err) = cause.downcast_ref::<Self>() {
                return err.clone();
            }
        }

        warn!("hyper request error: {err:?}");

        Self::HttpProtocolError
    }

    /// Translate a [`hyper::Error`] to a wasi-http [ErrorCode] in the context of a response.
    pub fn from_hyper_response_error(err: hyper::Error) -> Self {
        if err.is_timeout() {
            return ErrorCode::HttpResponseTimeout;
        }

        // If there's a source, we might be able to extract a wasi-http error from it.
        if let Some(cause) = err.source() {
            if let Some(err) = cause.downcast_ref::<Self>() {
                return err.clone();
            }
        }

        warn!("hyper response error: {err:?}");

        ErrorCode::HttpProtocolError
    }
}

impl From<Infallible> for ErrorCode {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

impl From<http::Method> for Method {
    fn from(method: http::Method) -> Self {
        Self::from(&method)
    }
}

impl From<&http::Method> for Method {
    fn from(method: &http::Method) -> Self {
        if method == http::Method::GET {
            Self::Get
        } else if method == http::Method::HEAD {
            Self::Head
        } else if method == http::Method::POST {
            Self::Post
        } else if method == http::Method::PUT {
            Self::Put
        } else if method == http::Method::DELETE {
            Self::Delete
        } else if method == http::Method::CONNECT {
            Self::Connect
        } else if method == http::Method::OPTIONS {
            Self::Options
        } else if method == http::Method::TRACE {
            Self::Trace
        } else if method == http::Method::PATCH {
            Self::Patch
        } else {
            Self::Other(method.as_str().into())
        }
    }
}

impl TryFrom<Method> for http::Method {
    type Error = http::method::InvalidMethod;

    fn try_from(method: Method) -> Result<Self, Self::Error> {
        Self::try_from(&method)
    }
}

impl TryFrom<&Method> for http::Method {
    type Error = http::method::InvalidMethod;

    fn try_from(method: &Method) -> Result<Self, Self::Error> {
        match method {
            Method::Get => Ok(Self::GET),
            Method::Head => Ok(Self::HEAD),
            Method::Post => Ok(Self::POST),
            Method::Put => Ok(Self::PUT),
            Method::Delete => Ok(Self::DELETE),
            Method::Connect => Ok(Self::CONNECT),
            Method::Options => Ok(Self::OPTIONS),
            Method::Trace => Ok(Self::TRACE),
            Method::Patch => Ok(Self::PATCH),
            Method::Other(s) => s.parse(),
        }
    }
}

impl From<http::uri::Scheme> for Scheme {
    fn from(scheme: http::uri::Scheme) -> Self {
        Self::from(&scheme)
    }
}

impl From<&http::uri::Scheme> for Scheme {
    fn from(scheme: &http::uri::Scheme) -> Self {
        if *scheme == http::uri::Scheme::HTTP {
            Self::Http
        } else if *scheme == http::uri::Scheme::HTTPS {
            Self::Https
        } else {
            Self::Other(scheme.as_str().into())
        }
    }
}

impl TryFrom<Scheme> for http::uri::Scheme {
    type Error = http::uri::InvalidUri;

    fn try_from(scheme: Scheme) -> Result<Self, Self::Error> {
        Self::try_from(&scheme)
    }
}

impl TryFrom<&Scheme> for http::uri::Scheme {
    type Error = http::uri::InvalidUri;

    fn try_from(scheme: &Scheme) -> Result<Self, Self::Error> {
        match scheme {
            Scheme::Http => Ok(Self::HTTP),
            Scheme::Https => Ok(Self::HTTPS),
            Scheme::Other(s) => s.parse(),
        }
    }
}

impl<T> From<http::Request<T>> for Request
where
    T: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    T::Error: Into<ErrorCode>,
{
    fn from(req: http::Request<T>) -> Self {
        let (
            http::request::Parts {
                method,
                uri,
                headers,
                ..
            },
            body,
        ) = req.into_parts();
        let http::uri::Parts {
            scheme,
            authority,
            path_and_query,
            ..
        } = uri.into_parts();
        Self::new(
            method,
            scheme,
            authority,
            path_and_query,
            headers,
            body,
            None,
        )
    }
}

impl<T> From<T> for Body
where
    T: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    T::Error: Into<ErrorCode>,
{
    fn from(body: T) -> Self {
        Self::new(body)
    }
}

impl TryFrom<Response> for http::Response<Body> {
    type Error = anyhow::Error;

    fn try_from(
        Response {
            status,
            headers,
            body,
            ..
        }: Response,
    ) -> Result<Self, Self::Error> {
        let headers = headers.unwrap_or_clone()?;
        let mut response = http::Response::builder().status(status);
        *response.headers_mut().unwrap() = headers;

        let Some(body) = Arc::into_inner(body) else {
            bail!("body is borrowed")
        };
        let Ok(body) = body.into_inner() else {
            bail!("lock poisoned");
        };
        response.body(body).context("failed to build response")
    }
}
