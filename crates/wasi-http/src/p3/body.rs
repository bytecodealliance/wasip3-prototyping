use crate::p3::ResourceView;
use crate::p3::bindings::http::types::ErrorCode;
use anyhow::Context as _;
use bytes::{Buf, Bytes};
use core::mem;
use core::pin::Pin;
use core::task::{Context, Poll, ready};
use http::HeaderMap;
use http_body::Frame;
use http_body_util::BodyExt as _;
use http_body_util::combinators::BoxBody;
use pin_project_lite::pin_project;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use wasmtime::AsContextMut;
use wasmtime::component::{
    Accessor, DropWithStore, DropWithStoreAndValue, FutureReader, FutureWriter, HasData, Resource,
    StreamReader,
};
use wasmtime_wasi::p3::WithChildren;

pub(crate) fn empty_body() -> impl http_body::Body<Data = Bytes, Error = Option<ErrorCode>> {
    http_body_util::Empty::new().map_err(|_| None)
}

/// Type for trailers that are received directly from the guest.
pub type GuestTrailers = Result<Option<Resource<WithChildren<HeaderMap>>>, ErrorCode>;

/// A body frame
pub enum BodyFrame {
    /// Data frame
    Data(Bytes),
    /// Trailer frame, this is the last frame of the body and it includes the transmit/receipt result
    Trailers(GuestTrailers),
}

/// Whether the body is a request or response body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyContext {
    /// The body is a request body.
    Request,
    /// The body is a response body.
    Response,
}

impl BodyContext {
    /// Construct the correct [`ErrorCode`] body size error.
    pub fn as_body_size_error(&self, size: u64) -> ErrorCode {
        match self {
            Self::Request => ErrorCode::HttpRequestBodySize(Some(size)),
            Self::Response => ErrorCode::HttpResponseBodySize(Some(size)),
        }
    }
}

/// The concrete type behind a `wasi:http/types/body` resource.
pub enum Body {
    /// Body constructed by the guest
    Guest {
        /// The body stream
        contents: MaybeTombstone<StreamReader<u8>>,
        /// Future, on which guest will write result and optional trailers
        trailers: MaybeTombstone<FutureReader<GuestTrailers>>,
        /// Buffered frame, if any
        buffer: Option<BodyFrame>,
        /// Future, on which transmission result will be written
        tx: FutureWriter<Result<(), ErrorCode>>,
        /// Optional `Content-Length` header limit and state
        content_length: Option<ContentLength>,
    },
    /// Body constructed by the host
    Host {
        /// Underlying body stream
        stream: Option<BoxBody<Bytes, ErrorCode>>,
        /// Buffered frame, if any
        buffer: Option<BodyFrame>,
    },
    /// Body has been fully consumed
    Consumed,
}

/// Variants of `Body::Guest::contents`.
pub enum MaybeTombstone<T> {
    /// The provided value is available.
    Some(T),

    /// The guest body item was previously taken into a body task, and that body
    /// task has finished.
    ///
    /// In this situation the guest body can no longer be read due to a bug in
    /// Wasmtime where cancellation of an in-progress read is not yet supported.
    Tombstone,

    /// The guest body is not provided.
    None,
}

impl Body {
    /// Construct a new [Body]
    pub fn new<T>(body: T) -> Self
    where
        T: http_body::Body<Data = Bytes> + Send + Sync + 'static,
        T::Error: Into<ErrorCode>,
    {
        Self::Host {
            stream: Some(body.map_err(Into::into).boxed()),
            buffer: None,
        }
    }

    /// Construct a new empty [Body]
    pub fn empty() -> Self {
        Self::Host {
            stream: Some(http_body_util::Empty::new().map_err(Into::into).boxed()),
            buffer: None,
        }
    }
}

impl DropWithStore for Body {
    fn drop(self, mut store: impl AsContextMut) -> wasmtime::Result<()> {
        if let Body::Guest {
            contents,
            trailers,
            tx,
            ..
        } = self
        {
            let mut store = store.as_context_mut();
            if let MaybeTombstone::Some(contents) = contents {
                contents.drop(&mut store)?;
            }
            if let MaybeTombstone::Some(trailers) = trailers {
                trailers.drop(&mut store)?;
            }
            tx.drop(store, Ok(()))?;
        }
        anyhow::Ok(())
    }
}

/// Represents `Content-Length` limit and state
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ContentLength {
    /// Limit of bytes to be sent
    pub limit: u64,
    /// Number of bytes sent
    pub sent: u64,
}

impl ContentLength {
    /// Constructs new [ContentLength]
    pub fn new(limit: u64) -> Self {
        Self { limit, sent: 0 }
    }
}

/// Response body constructed by the guest
pub(crate) struct OutgoingResponseBody {
    contents: Option<mpsc::Receiver<Bytes>>,
    buffer: Bytes,
    content_length: Option<ContentLength>,
}

impl OutgoingResponseBody {
    pub fn new(
        contents: mpsc::Receiver<Bytes>,
        buffer: Bytes,
        content_length: Option<ContentLength>,
    ) -> Self {
        Self {
            contents: Some(contents),
            buffer,
            content_length,
        }
    }
}

impl http_body::Body for OutgoingResponseBody {
    type Data = Bytes;
    type Error = Option<ErrorCode>;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        if !self.buffer.is_empty() {
            let buffer = mem::take(&mut self.buffer);
            if let Some(ContentLength { limit, sent }) = &mut self.content_length {
                let Ok(n) = buffer.len().try_into() else {
                    return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(None)))));
                };
                let Some(n) = sent.checked_add(n) else {
                    return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(None)))));
                };
                if n > *limit {
                    return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(Some(n))))));
                }
                *sent = n;
            }
            return Poll::Ready(Some(Ok(http_body::Frame::data(buffer))));
        }
        let Some(stream) = &mut self.contents else {
            return Poll::Ready(None);
        };
        match ready!(stream.poll_recv(cx)) {
            Some(buf) => {
                if let Some(ContentLength { limit, sent }) = &mut self.content_length {
                    let Ok(n) = buf.len().try_into() else {
                        return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(None)))));
                    };
                    let Some(n) = sent.checked_add(n) else {
                        return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(None)))));
                    };
                    if n > *limit {
                        return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(Some(
                            n,
                        ))))));
                    }
                    *sent = n;
                }
                Poll::Ready(Some(Ok(http_body::Frame::data(buf))))
            }
            None => {
                self.contents = None;
                if let Some(ContentLength { limit, sent }) = self.content_length {
                    if limit != sent {
                        return Poll::Ready(Some(Err(Some(ErrorCode::HttpRequestBodySize(Some(
                            sent,
                        ))))));
                    }
                }
                Poll::Ready(None)
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.contents.is_none()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        if let Some(ContentLength { limit, sent }) = self.content_length {
            http_body::SizeHint::with_exact(limit.saturating_sub(sent))
        } else {
            http_body::SizeHint::default()
        }
    }
}

/// Helper structure to validate that the body `B` provided matches the
/// content length specified in its header.
///
/// This will behave as if it were `B` except that an error will be
/// generated if too much data is generated or if too little data is
/// generated. This body will only succeed if the `body` contained produces
/// exactly `remaining` bytes.
pub(crate) struct BodyChannel<D, E> {
    rx: mpsc::Receiver<Result<D, E>>,
}

impl<D, E> BodyChannel<D, E> {
    pub(crate) fn new(rx: mpsc::Receiver<Result<D, E>>) -> Self {
        BodyChannel { rx }
    }
}

impl<D: Buf, E> http_body::Body for BodyChannel<D, E> {
    type Data = D;
    type Error = E;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(Ok(Frame::data(frame)))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pin_project! {
    /// Helper structure to validate that the body `B` provided matches the
    /// content length specified in its header.
    ///
    /// This will behave as if it were `B` except that an error will be
    /// generated if too much data is generated or if too little data is
    /// generated. This body will only succeed if the `body` contained produces
    /// exactly `remaining` bytes.
    pub(crate) struct BodyWithContentLength<B> {
        #[pin]
        body: B,
        content_length: Option<ContentLength>,
        body_length_mismatch: bool,
    }
}

impl<B> BodyWithContentLength<B> {
    pub(crate) fn new(body: B, content_length: Option<ContentLength>) -> BodyWithContentLength<B> {
        BodyWithContentLength {
            body,
            content_length,
            body_length_mismatch: false,
        }
    }
}

pub(crate) trait ContentLengthError: Sized {
    fn body_too_long(amt: Option<u64>) -> Self;
    fn body_too_short(amt: Option<u64>) -> Self;
}

impl<B> http_body::Body for BodyWithContentLength<B>
where
    B: http_body::Body,
    B::Error: ContentLengthError,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();
        if *this.body_length_mismatch {
            return Poll::Ready(None);
        }
        let frame = match Pin::new(&mut this.body).poll_frame(cx) {
            Poll::Ready(frame) => frame,
            Poll::Pending => return Poll::Pending,
        };
        let content_length = match &mut this.content_length {
            Some(content_length) => content_length,
            None => return Poll::Ready(frame),
        };
        let res = match frame {
            Some(Ok(frame)) => {
                if let Some(data) = frame.data_ref() {
                    let data_len = u64::try_from(data.remaining()).unwrap();
                    content_length.sent = content_length.sent.saturating_add(data_len);
                    if content_length.sent > content_length.limit {
                        *this.body_length_mismatch = true;
                        Some(Err(B::Error::body_too_long(Some(content_length.sent))))
                    } else {
                        Some(Ok(frame))
                    }
                } else {
                    Some(Ok(frame))
                }
            }
            Some(Err(err)) => Some(Err(err)),
            None => {
                if content_length.sent != content_length.limit {
                    *this.body_length_mismatch = true;
                    Some(Err(B::Error::body_too_short(Some(content_length.sent))))
                } else {
                    None
                }
            }
        };

        Poll::Ready(res)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        let mut hint = self.body.size_hint();
        if let Some(content_length) = self.content_length {
            let remaining = content_length.limit.saturating_sub(content_length.sent);
            if hint.lower() >= remaining {
                hint.set_exact(remaining)
            } else if let Some(max) = hint.upper() {
                hint.set_upper(remaining.min(max))
            } else {
                hint.set_upper(remaining)
            }
        }
        hint
    }
}

impl ContentLengthError for Option<ErrorCode> {
    fn body_too_long(amt: Option<u64>) -> Self {
        Some(ErrorCode::HttpRequestBodySize(amt))
    }
    fn body_too_short(amt: Option<u64>) -> Self {
        Some(ErrorCode::HttpRequestBodySize(amt))
    }
}

pub(crate) struct IncomingResponseBody {
    pub incoming: hyper::body::Incoming,
    pub timeout: tokio::time::Interval,
}

impl http_body::Body for IncomingResponseBody {
    type Data = <hyper::body::Incoming as http_body::Body>::Data;
    type Error = ErrorCode;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match Pin::new(&mut self.as_mut().incoming).poll_frame(cx) {
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(err))) => {
                Poll::Ready(Some(Err(ErrorCode::from_hyper_response_error(err))))
            }
            Poll::Ready(Some(Ok(frame))) => {
                self.timeout.reset();
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending => {
                ready!(self.timeout.poll_tick(cx));
                Poll::Ready(Some(Err(ErrorCode::ConnectionReadTimeout)))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.incoming.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.incoming.size_hint()
    }
}

pub(crate) async fn handle_guest_trailers<T, D>(
    store: &Accessor<T, D>,
    guest_trailers: FutureReader<GuestTrailers>,
    host_trailers: oneshot::Sender<Result<http::HeaderMap, Option<ErrorCode>>>,
) -> wasmtime::Result<()>
where
    D: HasData,
    for<'a> D::Data<'a>: ResourceView,
{
    match guest_trailers.read(store).await {
        Some(Ok(Some(trailers))) => {
            let trailers = store.with(|mut store| {
                let mut binding = store.get();
                let table = binding.table();
                table.delete(trailers).context("failed to delete trailers")
            })?;
            let trailers = trailers.unwrap_or_clone()?;
            _ = host_trailers.send(Ok(trailers));
        }
        Some(Err(err)) => {
            _ = host_trailers.send(Err(Some(err)));
        }
        // If no trailers were explicitly sent, or if nothing was sent at all,
        // then interpret that as no trailers.
        Some(Ok(None)) | None => {}
    }
    Ok(())
}
