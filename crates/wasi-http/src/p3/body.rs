use core::future::{poll_fn, Future};
use core::mem;
use core::pin::Pin;
use core::task::{ready, Context, Poll};

use anyhow::Context as _;
use bytes::Bytes;
use http::HeaderMap;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt as _};
use tokio::sync::oneshot;
use wasmtime::{
    component::{AbortOnDropHandle, ErrorContext, FutureWriter, Resource, StreamReader},
    AsContextMut,
};
use wasmtime_wasi::p3::{ResourceView, WithChildren};

use crate::p3::bindings::http::types::ErrorCode;

pub(crate) type OutgoingContentsStreamFuture = Pin<
    Box<
        dyn Future<Output = Result<(StreamReader<Bytes>, Bytes), Option<ErrorContext>>>
            + Send
            + 'static,
    >,
>;

pub(crate) type OutgoingTrailerFuture = Pin<
    Box<
        dyn Future<
                Output = Result<
                    Result<Option<Resource<WithChildren<HeaderMap>>>, ErrorCode>,
                    Option<ErrorContext>,
                >,
            > + Send
            + 'static,
    >,
>;

pub(crate) type OutgoingTrailerFutureMut<'a> = Pin<
    &'a mut (dyn Future<
        Output = Result<
            Result<Option<Resource<WithChildren<HeaderMap>>>, ErrorCode>,
            Option<ErrorContext>,
        >,
    > + Send
                 + 'static),
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

pub(crate) struct GuestRequestTrailers {
    pub trailers: Option<oneshot::Receiver<Result<Option<HeaderMap>, ErrorCode>>>,
    #[allow(dead_code)]
    pub trailer_task: AbortOnDropHandle,
}

impl Future for GuestRequestTrailers {
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

fn poll_guest_response_trailers<T: ResourceView>(
    cx: &mut Context<'_>,
    mut store: impl AsContextMut<Data = T>,
    trailers: OutgoingTrailerFutureMut<'_>,
) -> Poll<Option<Result<HeaderMap, Option<ErrorCode>>>> {
    match ready!(trailers.poll(cx)) {
        Ok(Ok(Some(trailers))) => {
            let mut store = store.as_context_mut();
            let table = store.data_mut().table();
            match table
                .delete(trailers)
                .context("failed to delete trailers")
                .map(WithChildren::unwrap_or_clone)
            {
                Ok(Ok(trailers)) => Poll::Ready(Some(Ok(trailers))),
                Ok(Err(err)) => Poll::Ready(Some(Err(Some(ErrorCode::InternalError(Some(
                    format!("{err:#}"),
                )))))),
                Err(err) => Poll::Ready(Some(Err(Some(ErrorCode::InternalError(Some(format!(
                    "{err:#}"
                ))))))),
            }
        }
        Ok(Ok(None)) => Poll::Ready(None),
        Ok(Err(err)) => Poll::Ready(Some(Err(Some(err)))),
        Err(..) => Poll::Ready(Some(Err(None))),
    }
}

pub(crate) async fn guest_response_trailers<T>(
    mut store: impl AsContextMut<Data = T>,
    mut trailers: OutgoingTrailerFuture,
) -> Option<Result<HeaderMap, Option<ErrorCode>>>
where
    T: ResourceView,
{
    poll_fn(move |cx| poll_guest_response_trailers(cx, &mut store, trailers.as_mut())).await
}

pub(crate) struct GuestResponseTrailers<T> {
    pub store: T,
    pub trailers: Option<OutgoingTrailerFuture>,
}

impl<T> Future for GuestResponseTrailers<T>
where
    T: AsContextMut + Unpin,
    T::Data: ResourceView,
{
    type Output = Option<Result<HeaderMap, Option<ErrorCode>>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let &mut Self {
            ref mut store,
            trailers: Some(ref mut trailers),
        } = &mut *self.as_mut()
        else {
            return Poll::Ready(None);
        };
        let trailers = ready!(poll_guest_response_trailers(cx, store, trailers.as_mut()));
        self.trailers = None;
        Poll::Ready(trailers)
    }
}

/// Body constructed by the guest
pub(crate) struct GuestBody {
    pub contents: Option<OutgoingContentsStreamFuture>,
    pub buffer: Bytes,
}

impl GuestBody {
    pub fn new(contents: OutgoingContentsStreamFuture, buffer: Bytes) -> Self {
        Self {
            contents: Some(contents),
            buffer,
        }
    }
}

impl http_body::Body for GuestBody {
    type Data = Bytes;
    type Error = Option<ErrorCode>;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        if !self.buffer.is_empty() {
            let buffer = mem::take(&mut self.buffer);
            return Poll::Ready(Some(Ok(http_body::Frame::data(buffer))));
        }
        let Some(stream) = &mut self.contents else {
            return Poll::Ready(None);
        };
        match ready!(Pin::new(stream).poll(cx)) {
            Ok((tail, buf)) => {
                self.contents = Some(tail.read().into_future());
                Poll::Ready(Some(Ok(http_body::Frame::data(buf))))
            }
            Err(..) => {
                self.contents = None;
                Poll::Ready(None)
            }
        }
    }
}
