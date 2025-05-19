use core::future::Future;
use core::mem;
use core::pin::Pin;
use core::task::{Context, Poll, ready};

use bytes::{Bytes, BytesMut};
use http::HeaderMap;
use http_body_util::BodyExt as _;
use http_body_util::combinators::UnsyncBoxBody;
use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{AbortOnDropHandle, FutureWriter, Resource, StreamReader};
use wasmtime_wasi::p3::WithChildren;

use crate::p3::DEFAULT_BUFFER_CAPACITY;
use crate::p3::bindings::http::types::ErrorCode;

pub(crate) type OutgoingContentsStreamFuture =
    Pin<Box<dyn Future<Output = (Option<StreamReader<BytesMut>>, BytesMut)> + Send + 'static>>;

pub(crate) type OutgoingTrailerFuture = Pin<
    Box<
        dyn Future<Output = Option<Result<Option<Resource<WithChildren<HeaderMap>>>, ErrorCode>>>
            + Send
            + 'static,
    >,
>;

pub(crate) fn empty_body() -> impl http_body::Body<Data = Bytes, Error = Option<ErrorCode>> {
    http_body_util::Empty::new().map_err(|_| None)
}

/// A body frame
pub enum BodyFrame {
    /// Data frame
    Data(Bytes),
    /// Trailer frame, this is the last frame of the body and it includes the transmit/receipt result
    Trailers(Result<Option<Resource<WithChildren<HeaderMap>>>, ErrorCode>),
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
        contents: Option<OutgoingContentsStreamFuture>,
        /// Future, on which guest will write result and optional trailers
        trailers: Option<OutgoingTrailerFuture>,
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
        stream: Option<UnsyncBoxBody<Bytes, ErrorCode>>,
        /// Buffered frame, if any
        buffer: Option<BodyFrame>,
    },
    /// Body has been fully consumed
    Consumed,
}

impl Body {
    /// Construct a new [Body]
    pub fn new<T>(body: T) -> Self
    where
        T: http_body::Body<Data = Bytes> + Send + 'static,
        T::Error: Into<ErrorCode>,
    {
        Self::Host {
            stream: Some(body.map_err(Into::into).boxed_unsync()),
            buffer: None,
        }
    }

    /// Construct a new empty [Body]
    pub fn empty() -> Self {
        Self::Host {
            stream: Some(
                http_body_util::Empty::new()
                    .map_err(Into::into)
                    .boxed_unsync(),
            ),
            buffer: None,
        }
    }
}

pub(crate) struct OutgoingRequestTrailers {
    pub trailers: Option<oneshot::Receiver<Result<Option<HeaderMap>, ErrorCode>>>,
    #[expect(dead_code, reason = "here for the dtor")]
    pub trailer_task: AbortOnDropHandle,
}

impl Future for OutgoingRequestTrailers {
    type Output = Option<Result<HeaderMap, Option<ErrorCode>>>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<HeaderMap, Option<ErrorCode>>>> {
        let Some(trailers) = &mut self.trailers else {
            return Poll::Ready(None);
        };
        let trailers = ready!(Pin::new(trailers).poll(cx));
        self.trailers = None;
        match trailers {
            Ok(Ok(Some(trailers))) => Poll::Ready(Some(Ok(trailers))),
            Ok(Ok(None)) => Poll::Ready(None),
            Ok(Err(err)) => Poll::Ready(Some(Err(Some(err)))),
            Err(..) => Poll::Ready(Some(Err(None))), // future was dropped without writing a result
        }
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

/// Request body constructed by the guest
pub(crate) struct OutgoingRequestBody {
    pub contents: Option<OutgoingContentsStreamFuture>,
    pub buffer: Bytes,
    pub content_length: Option<ContentLength>,
}

impl OutgoingRequestBody {
    pub fn new(
        contents: OutgoingContentsStreamFuture,
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

impl http_body::Body for OutgoingRequestBody {
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
        let (tail, mut rx_buffer) = ready!(Pin::new(stream).poll(cx));
        match tail {
            Some(tail) => {
                let buffer = rx_buffer.split();
                rx_buffer.reserve(DEFAULT_BUFFER_CAPACITY);
                self.contents = Some(Box::pin(tail.read(rx_buffer)));
                if let Some(ContentLength { limit, sent }) = &mut self.content_length {
                    let Ok(n) = buffer.len().try_into() else {
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
                Poll::Ready(Some(Ok(http_body::Frame::data(buffer.freeze()))))
            }
            None => {
                debug_assert!(rx_buffer.is_empty());
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
