use core::iter;

use std::sync::Arc;

use anyhow::{bail, Context as _};
use bytes::Bytes;
use futures::StreamExt as _;
use http::{HeaderMap, StatusCode};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, BodyStream, StreamBody};
use wasmtime::component::{AbortOnDropHandle, FutureWriter};
use wasmtime::AsContextMut;
use wasmtime_wasi::p3::{ResourceView, WithChildren};

use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{
    empty_body, guest_response_trailers, Body, BodyContext, BodyFrame, ContentLength, GuestBody,
};

/// The concrete type behind a `wasi:http/types/response` resource.
pub struct Response {
    /// The status of the response.
    pub status: StatusCode,
    /// The headers of the response.
    pub headers: WithChildren<HeaderMap>,
    /// The body of the response.
    pub(crate) body: Arc<std::sync::Mutex<Body>>,
    /// Body stream task handle
    pub(crate) body_task: Option<AbortOnDropHandle>,
}

impl Response {
    /// Construct a new [Response]
    pub fn new(status: StatusCode, headers: HeaderMap, body: Body) -> Self {
        Self {
            status,
            headers: WithChildren::new(headers),
            body: Arc::new(std::sync::Mutex::new(body)),
            body_task: None,
        }
    }

    /// Convert [Response] into [http::Response].
    pub fn into_http<T: ResourceView + 'static>(
        self,
        mut store: impl AsContextMut<Data = T> + Send + 'static,
    ) -> anyhow::Result<(
        http::Response<UnsyncBoxBody<Bytes, Option<ErrorCode>>>,
        Option<FutureWriter<Result<(), ErrorCode>>>,
    )> {
        let headers = self.headers.unwrap_or_clone()?;
        let mut response = http::Response::builder().status(self.status);
        *response.headers_mut().unwrap() = headers;
        let response = response.body(()).context("failed to build response")?;

        let Some(body) = Arc::into_inner(self.body) else {
            bail!("body is borrowed")
        };
        let Ok(body) = body.into_inner() else {
            bail!("lock poisoned");
        };
        let (body, tx) = match body {
            Body::Guest {
                contents: None,
                buffer: None | Some(BodyFrame::Trailers(Ok(None))),
                content_length: Some(ContentLength { limit, sent }),
                ..
            } if limit != sent => {
                bail!("guest response Content-Length mismatch, limit: {limit}, sent: {sent}")
            }
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
                tx,
                ..
            } => (empty_body().boxed_unsync(), Some(tx)),
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Ok(Some(trailers)))),
                tx,
                ..
            } => {
                let mut store = store.as_context_mut();
                let table = store.data_mut().table();
                let trailers = table
                    .delete(trailers)
                    .context("failed to delete trailers")?;
                let trailers = trailers.unwrap_or_clone()?;
                (
                    empty_body()
                        .with_trailers(async move { Some(Ok(trailers)) })
                        .boxed_unsync(),
                    Some(tx),
                )
            }
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
                tx,
                ..
            } => (
                empty_body()
                    .with_trailers(async move { Some(Err(Some(err))) })
                    .boxed_unsync(),
                Some(tx),
            ),
            Body::Guest {
                contents: None,
                trailers: Some(trailers),
                buffer: None,
                tx,
                ..
            } => {
                let body = empty_body()
                    .with_trailers(guest_response_trailers(store, trailers))
                    .boxed_unsync();
                (body, Some(tx))
            }
            Body::Guest {
                contents: Some(contents),
                trailers: Some(trailers),
                buffer,
                tx,
                content_length,
            } => {
                let buffer = match buffer {
                    Some(BodyFrame::Data(buffer)) => buffer,
                    Some(BodyFrame::Trailers(..)) => bail!("guest body is corrupted"),
                    None => Bytes::default(),
                };
                let body = GuestBody::new(BodyContext::Response, contents, buffer, content_length)
                    .with_trailers(guest_response_trailers(store, trailers))
                    .boxed_unsync();
                (body, Some(tx))
            }
            Body::Guest { .. } => bail!("guest body is corrupted"),
            Body::Consumed
            | Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
            } => (empty_body().boxed_unsync(), None),
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Ok(Some(trailers)))),
            } => {
                let mut store = store.as_context_mut();
                let table = store.data_mut().table();
                let trailers = table
                    .delete(trailers)
                    .context("failed to delete trailers")?;
                let trailers = trailers.unwrap_or_clone()?;
                (
                    empty_body()
                        .with_trailers(async move { Some(Ok(trailers)) })
                        .boxed_unsync(),
                    None,
                )
            }
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
            } => (
                empty_body()
                    .with_trailers(async move { Some(Err(Some(err))) })
                    .boxed_unsync(),
                None,
            ),
            Body::Host {
                stream: Some(stream),
                buffer: None,
            } => (stream.map_err(Some).boxed_unsync(), None),
            Body::Host {
                stream: Some(stream),
                buffer: Some(BodyFrame::Data(buffer)),
            } => {
                let buffer = futures::stream::iter(iter::once(Ok(http_body::Frame::data(buffer))));
                (
                    BodyExt::boxed_unsync(StreamBody::new(
                        buffer.chain(BodyStream::new(stream.map_err(Some))),
                    )),
                    None,
                )
            }
            Body::Host { .. } => bail!("host body is corrupted"),
        };
        Ok((response.map(|()| body), tx))
    }
}
