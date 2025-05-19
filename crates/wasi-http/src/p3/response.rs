use core::iter;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{bail, Context as _};
use bytes::Bytes;
use futures::{FutureExt as _, StreamExt as _};
use http::{HeaderMap, StatusCode};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, BodyStream, StreamBody};
use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{AbortOnDropHandle, FutureWriter, Resource};
use wasmtime::{AsContextMut, StoreContextMut};
use wasmtime_wasi::p3::{ResourceView, WithChildren};

use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{
    empty_body, Body, BodyFrame, ContentLength, OutgoingResponseBody, OutgoingTrailerFuture,
    DEFAULT_BUFFER_CAPACITY,
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

async fn receive_trailers(
    rx: oneshot::Receiver<Option<Result<WithChildren<HeaderMap>, ErrorCode>>>,
) -> Option<Result<HeaderMap, Option<ErrorCode>>> {
    match rx.await {
        Ok(Some(Ok(trailers))) => match trailers.unwrap_or_clone() {
            Ok(trailers) => Some(Ok(trailers)),
            Err(err) => Some(Err(Some(ErrorCode::InternalError(Some(format!(
                "{err:#}"
            )))))),
        },
        Ok(Some(Err(err))) => Some(Err(Some(err))),
        Ok(None) => None,
        Err(..) => Some(Err(None)),
    }
}

async fn handle_guest_trailers<T: ResourceView + 'static>(
    rx: OutgoingTrailerFuture,
    tx: oneshot::Sender<Option<Result<WithChildren<HeaderMap>, ErrorCode>>>,
) -> ResponsePromiseClosure<T> {
    let Some(trailers) = rx.await else {
        return Box::new(|_| Ok(()));
    };
    match trailers {
        Ok(Some(trailers)) => Box::new(|mut store| {
            let table = store.data_mut().table();
            let trailers = table
                .delete(trailers)
                .context("failed to delete trailers")?;
            _ = tx.send(Some(Ok(trailers)));
            Ok(())
        }),
        Ok(None) => Box::new(|_| {
            _ = tx.send(None);
            Ok(())
        }),
        Err(err) => Box::new(|_| {
            _ = tx.send(Some(Err(err)));
            Ok(())
        }),
    }
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
        http::Response<UnsyncBoxBody<Bytes, Option<ErrorCode>>>,
        Option<FutureWriter<Result<(), ErrorCode>>>,
        Option<Pin<Box<dyn Future<Output = ResponsePromiseClosure<T>> + Send + 'static>>>,
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
        http::Response<UnsyncBoxBody<Bytes, Option<ErrorCode>>>,
        Option<FutureWriter<Result<(), ErrorCode>>>,
        Option<Pin<Box<dyn Future<Output = ResponsePromiseClosure<T>> + Send + 'static>>>,
    )> {
        let response = http::Response::try_from(self)?;
        let (response, body) = response.into_parts();
        let (body, tx, promise) = match body {
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
            } => (empty_body().boxed_unsync(), Some(tx), None),
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
                    None,
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
                None,
            ),
            Body::Guest {
                contents: None,
                trailers: Some(trailers),
                buffer: None,
                tx,
                ..
            } => {
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let body = empty_body()
                    .with_trailers(receive_trailers(trailers_rx))
                    .boxed_unsync();
                let fut = handle_guest_trailers(trailers, trailers_tx).boxed();
                (body, Some(tx), Some(fut))
            }
            Body::Guest {
                contents: Some(mut contents),
                trailers: Some(trailers),
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
                    .with_trailers(receive_trailers(trailers_rx))
                    .boxed_unsync();
                let fut = async move {
                    loop {
                        let (tail, mut rx_buffer) = contents.await;
                        if let Some(tail) = tail {
                            let buffer = rx_buffer.split();
                            if !buffer.is_empty() {
                                if let Err(..) = contents_tx.send(buffer.freeze()).await {
                                    break;
                                }
                                rx_buffer.reserve(DEFAULT_BUFFER_CAPACITY);
                            }
                            contents = tail.read(rx_buffer).boxed();
                        } else {
                            debug_assert!(rx_buffer.is_empty());
                            break;
                        }
                    }
                    drop(contents_tx);
                    handle_guest_trailers(trailers, trailers_tx).await
                }
                .boxed();
                (body, Some(tx), Some(fut))
            }
            Body::Guest { .. } => bail!("guest body is corrupted"),
            Body::Consumed
            | Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
            } => (empty_body().boxed_unsync(), None, None),
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
                None,
            ),
            Body::Host {
                stream: Some(stream),
                buffer: None,
            } => (stream.map_err(Some).boxed_unsync(), None, None),
            Body::Host {
                stream: Some(stream),
                buffer: Some(BodyFrame::Data(buffer)),
            } => (
                BodyExt::boxed_unsync(StreamBody::new(
                    futures::stream::iter(iter::once(Ok(http_body::Frame::data(buffer))))
                        .chain(BodyStream::new(stream.map_err(Some))),
                )),
                None,
                None,
            ),
            Body::Host { .. } => bail!("host body is corrupted"),
        };
        Ok((http::Response::from_parts(response, body), tx, promise))
    }
}
