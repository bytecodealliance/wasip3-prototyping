use core::iter;

use std::future::Future;
use std::sync::Arc;

use anyhow::{Context as _, bail};
use bytes::{Bytes, BytesMut};
use futures::StreamExt as _;
use http::{HeaderMap, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, BodyStream, StreamBody};
use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{Accessor, FutureReader, FutureWriter, Resource, StreamReader};
use wasmtime::{AsContextMut, StoreContextMut};
use wasmtime_wasi::p3::{AbortOnDropHandle, ResourceView, WithChildren};

use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{
    Body, BodyFrame, ContentLength, DEFAULT_BUFFER_CAPACITY, GuestTrailers, MaybeTombstone,
    OutgoingResponseBody, empty_body, handle_guest_trailers,
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

/// Closure returned by promise returned by [`Response::into_http`]
pub type ResponsePromiseClosure<T> =
    Box<dyn for<'a> FnOnce(StoreContextMut<'a, T>) -> wasmtime::Result<()> + Send + Sync + 'static>;

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

    /// Delete [Response] from table associated with `T`
    /// and call [Self::into_http].
    /// See [Self::into_http] for documentation on return values of this function.
    pub fn resource_into_http<T>(
        mut store: impl AsContextMut<Data = T>,
        res: Resource<Response>,
    ) -> wasmtime::Result<(
        http::Response<BoxBody<Bytes, Option<ErrorCode>>>,
        ResponseIo,
    )>
    where
        T: ResourceView + Send + 'static,
    {
        let mut store = store.as_context_mut();
        let res = store
            .data_mut()
            .table()
            .delete(res)
            .context("failed to delete response from table")?;
        res.into_http(store)
    }

    /// Convert [Response] into [http::Response].
    /// This function will return a [`FutureWriter`], if the response was created
    /// by the guest using `wasi:http/types#[constructor]response.new`
    /// This function may return a [`Promise`], which must be awaited
    /// to drive I/O for bodies originating from the guest.
    pub fn into_http<T: ResourceView + Send + 'static>(
        self,
        mut store: impl AsContextMut<Data = T>,
    ) -> anyhow::Result<(
        http::Response<BoxBody<Bytes, Option<ErrorCode>>>,
        ResponseIo,
    )> {
        let response = http::Response::try_from(self)?;
        let (response, body) = response.into_parts();
        let (body, io) = match body {
            Body::Guest {
                contents: MaybeTombstone::None,
                buffer: None | Some(BodyFrame::Trailers(Ok(None))),
                content_length: Some(ContentLength { limit, sent }),
                ..
            } if limit != sent => {
                bail!("guest response Content-Length mismatch, limit: {limit}, sent: {sent}")
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
                tx,
                ..
            } => (empty_body().boxed(), ResponseIo::new_only_tx(tx)),
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
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
                        .boxed(),
                    ResponseIo::new_only_tx(tx),
                )
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
                tx,
                ..
            } => (
                empty_body()
                    .with_trailers(async move { Some(Err(Some(err))) })
                    .boxed(),
                ResponseIo::new_only_tx(tx),
            ),
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::Some(trailers),
                buffer: None,
                tx,
                ..
            } => {
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let body = empty_body()
                    .with_trailers(async { trailers_rx.await.ok() })
                    .boxed();
                (
                    body,
                    ResponseIo {
                        body: None,
                        tx: Some(tx),
                        trailers: Some((trailers, trailers_tx)),
                    },
                )
            }
            Body::Guest {
                contents: MaybeTombstone::Some(contents),
                trailers: MaybeTombstone::Some(trailers),
                buffer,
                tx,
                content_length,
            } => {
                let (contents_tx, contents_rx) = mpsc::channel(1);
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let buffer = match buffer {
                    Some(BodyFrame::Data(buffer)) => buffer,
                    Some(BodyFrame::Trailers(..)) => bail!("guest body is corrupted"),
                    None => Bytes::default(),
                };

                let body = OutgoingResponseBody::new(contents_rx, buffer, content_length)
                    .with_trailers(async { trailers_rx.await.ok() })
                    .boxed();
                (
                    body,
                    ResponseIo {
                        body: Some((contents, contents_tx)),
                        tx: Some(tx),
                        trailers: Some((trailers, trailers_tx)),
                    },
                )
            }
            Body::Guest { .. } => bail!("guest body is corrupted"),
            Body::Consumed
            | Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
            } => (empty_body().boxed(), ResponseIo::none()),
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
                        .boxed(),
                    ResponseIo::none(),
                )
            }
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
            } => (
                empty_body()
                    .with_trailers(async move { Some(Err(Some(err))) })
                    .boxed(),
                ResponseIo::none(),
            ),
            Body::Host {
                stream: Some(stream),
                buffer: None,
            } => (stream.map_err(Some).boxed(), ResponseIo::none()),
            Body::Host {
                stream: Some(stream),
                buffer: Some(BodyFrame::Data(buffer)),
            } => (
                BodyExt::boxed(StreamBody::new(
                    futures::stream::iter(iter::once(Ok(http_body::Frame::data(buffer))))
                        .chain(BodyStream::new(stream.map_err(Some))),
                )),
                ResponseIo::none(),
            ),
            Body::Host { .. } => bail!("host body is corrupted"),
        };
        Ok((http::Response::from_parts(response, body), io))
    }
}

/// Return structure from [`Response::resource_into_http`] and
/// [`Resource::into_http`] to perform I/O in the store.
///
/// This is primarily used with its [`ResponseIo::run`] method to finish guest
/// I/O for the body, if necessary.
pub struct ResponseIo {
    body: Option<(StreamReader<BytesMut>, mpsc::Sender<Bytes>)>,
    trailers: Option<(
        FutureReader<GuestTrailers>,
        oneshot::Sender<Result<HeaderMap, Option<ErrorCode>>>,
    )>,
    tx: Option<FutureWriter<Result<(), ErrorCode>>>,
}

impl ResponseIo {
    fn none() -> ResponseIo {
        ResponseIo {
            body: None,
            tx: None,
            trailers: None,
        }
    }

    fn new_only_tx(tx: FutureWriter<Result<(), ErrorCode>>) -> ResponseIo {
        ResponseIo {
            body: None,
            tx: Some(tx),
            trailers: None,
        }
    }

    /// Runs the body of this response's I/O, namely forwarding the guest
    /// body/trailers.
    ///
    /// The provided `io_result` is transmitted to the guest once I/O is
    /// complete.
    pub async fn run<T>(
        mut self,
        accessor: &Accessor<T>,
        io_result: impl Future<Output = Result<(), ErrorCode>>,
    ) -> wasmtime::Result<()>
    where
        T: ResourceView + 'static,
    {
        if let Some((contents, contents_tx)) = self.body.take() {
            let (mut tail, mut rx_buffer) = contents
                .read(BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY))
                .await;
            loop {
                let buffer = rx_buffer.split();
                if !buffer.is_empty() {
                    if let Err(..) = contents_tx.send(buffer.freeze()).await {
                        break;
                    }
                    rx_buffer.reserve(DEFAULT_BUFFER_CAPACITY);
                }
                if let Some(rx) = tail {
                    (tail, rx_buffer) = rx.read(rx_buffer).await;
                } else {
                    debug_assert!(rx_buffer.is_empty());
                    break;
                }
            }
            drop(contents_tx);
        }

        if let Some((trailers, trailers_tx)) = self.trailers.take() {
            handle_guest_trailers(accessor, trailers, trailers_tx).await?;
        }

        let io_result = io_result.await;
        if let Some(tx) = self.tx.take() {
            tx.write(io_result).await;
        }
        Ok(())
    }
}
