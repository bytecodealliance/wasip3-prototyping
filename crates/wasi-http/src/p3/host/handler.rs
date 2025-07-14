use super::{delete_request, get_fields_inner, push_response};
use crate::p3::bindings::http::handler;
use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{
    Body, BodyFrame, Client as _, ContentLength, OutgoingRequestBody, OutgoingRequestTrailers,
    OutgoingTrailerFuture, Request, Response, WasiHttp, WasiHttpImpl, WasiHttpView, empty_body,
};
use anyhow::bail;
use bytes::Bytes;
use core::iter;
use futures::StreamExt as _;
use http::header::HOST;
use http::{HeaderValue, Uri};
use http_body_util::{BodyExt as _, BodyStream, StreamBody};
use std::sync::Arc;
use tokio::sync::oneshot;
use tracing::debug;
use wasmtime::component::{Accessor, AccessorTask, Resource};
use wasmtime_wasi::p3::{AbortOnDropHandle, AccessorTaskFn, ResourceView as _};

struct TrailerTask {
    rx: OutgoingTrailerFuture,
    tx: oneshot::Sender<Result<Option<http::HeaderMap>, ErrorCode>>,
}

impl<T, U: WasiHttpView> AccessorTask<T, WasiHttp<U>, wasmtime::Result<()>> for TrailerTask
where
    U: 'static,
{
    async fn run(self, store: &mut Accessor<T, WasiHttp<U>>) -> wasmtime::Result<()> {
        match self.rx.await {
            Some(Ok(trailers)) => store.with(|mut view| {
                let mut binding = view.get();
                let trailers = trailers
                    .map(|trailers| get_fields_inner(binding.table(), &trailers))
                    .transpose()?;
                _ = self.tx.send(Ok(trailers.as_deref().cloned()));
                Ok(())
            }),
            Some(Err(err)) => {
                _ = self.tx.send(Err(err));
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl<T> handler::HostConcurrent for WasiHttp<T>
where
    T: WasiHttpView + 'static,
{
    async fn handle<U: 'static>(
        store: &mut Accessor<U, Self>,
        request: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<Response>, ErrorCode>> {
        let Request {
            method,
            scheme,
            authority,
            path_with_query,
            headers,
            body,
            options,
            ..
        } = store.with(|mut view| delete_request(view.get().table(), request))?;

        let mut client = store.with(|mut view| view.get().http().client.clone());

        let options = options
            .map(|options| options.unwrap_or_clone())
            .transpose()?;
        let mut headers = headers.unwrap_or_clone()?;
        if client.set_host_header() {
            let host = if let Some(authority) = authority.as_ref() {
                match HeaderValue::try_from(authority.as_str()) {
                    Ok(host) => host,
                    Err(err) => return Ok(Err(ErrorCode::InternalError(Some(err.to_string())))),
                }
            } else {
                HeaderValue::from_static("")
            };
            headers.insert(HOST, host);
        }

        let scheme = match scheme {
            None => client
                .default_scheme()
                .ok_or(ErrorCode::HttpProtocolError)?,
            Some(scheme) if client.is_supported_scheme(&scheme) => scheme,
            Some(..) => return Ok(Err(ErrorCode::HttpProtocolError)),
        };
        let mut uri = Uri::builder().scheme(scheme);
        if let Some(authority) = authority {
            uri = uri.authority(authority)
        };
        if let Some(path_with_query) = path_with_query {
            uri = uri.path_and_query(path_with_query)
        };
        let uri = match uri.build() {
            Ok(uri) => uri,
            Err(err) => {
                debug!(?err, "failed to build request URI");
                return Ok(Err(ErrorCode::HttpRequestUriInvalid));
            }
        };

        let Some(body) = Arc::into_inner(body) else {
            return Ok(Err(ErrorCode::InternalError(Some(
                "body is borrowed".into(),
            ))));
        };
        let Ok(body) = body.into_inner() else {
            bail!("lock poisoned");
        };

        let mut request = http::Request::builder();
        *request.headers_mut().unwrap() = headers;
        let request = match request.method(method).uri(uri).body(()) {
            Ok(request) => request,
            Err(err) => return Ok(Err(ErrorCode::InternalError(Some(err.to_string())))),
        };
        let (request, ()) = request.into_parts();
        let response = match body {
            Body::Guest {
                contents: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))) | None,
                tx,
                content_length: Some(ContentLength { limit, sent }),
                ..
            } if limit != sent => {
                store.spawn(AccessorTaskFn(
                    move |_: &mut Accessor<U, Self>| async move {
                        tx.write(Err(ErrorCode::HttpRequestBodySize(Some(sent))))
                            .await;
                        Ok(())
                    },
                ));
                return Ok(Err(ErrorCode::HttpRequestBodySize(Some(sent))));
            }
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
                tx,
                content_length: None,
            } => {
                let body = empty_body();
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            let res = io.await;
                            tx.write(res.map_err(Into::into)).await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Ok(Some(trailers)))),
                tx,
                content_length: None,
            } => {
                let trailers = store.with(|mut view| {
                    let trailers = get_fields_inner(view.get().table(), &trailers)?.clone();
                    anyhow::Ok(trailers)
                })?;
                let body = empty_body().with_trailers(async move { Some(Ok(trailers)) });
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            let res = io.await;
                            tx.write(res.map_err(Into::into)).await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
                tx,
                content_length: None,
            } => {
                store.spawn({
                    let err = err.clone();
                    AccessorTaskFn(move |_: &mut Accessor<U, Self>| async move {
                        tx.write(Err(err)).await;
                        Ok(())
                    })
                });
                return Ok(Err(err));
            }
            Body::Guest {
                contents: None,
                trailers: Some(trailers),
                buffer: None,
                tx,
                content_length: None,
            } => {
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let task = store.spawn(TrailerTask {
                    rx: trailers,
                    tx: trailers_tx,
                });
                let body = empty_body().with_trailers(OutgoingRequestTrailers {
                    trailers: Some(trailers_rx),
                    trailer_task: AbortOnDropHandle(task),
                });
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            let res = io.await;
                            tx.write(res.map_err(Into::into)).await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest {
                contents: Some(contents),
                trailers: Some(trailers),
                buffer,
                tx,
                content_length,
            } => {
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let task = store.spawn(TrailerTask {
                    rx: trailers,
                    tx: trailers_tx,
                });
                let buffer = match buffer {
                    Some(BodyFrame::Data(buffer)) => buffer,
                    Some(BodyFrame::Trailers(..)) => bail!("guest body is corrupted"),
                    None => Bytes::default(),
                };
                let body = OutgoingRequestBody::new(contents, buffer, content_length)
                    .with_trailers(OutgoingRequestTrailers {
                        trailers: Some(trailers_rx),
                        trailer_task: AbortOnDropHandle(task),
                    });
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            let res = io.await;
                            tx.write(res.map_err(Into::into)).await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),

                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest { .. } => bail!("guest body is corrupted"),
            Body::Host {
                stream: Some(stream),
                buffer: None,
            } => {
                let body = stream.map_err(Some);
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Host {
                stream: Some(stream),
                buffer: Some(BodyFrame::Data(buffer)),
            } => {
                let buffer = futures::stream::iter(iter::once(Ok(http_body::Frame::data(buffer))));
                let body = StreamBody::new(buffer.chain(BodyStream::new(stream.map_err(Some))));
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Ok(Some(trailers)))),
            } => {
                let trailers = store.with(|mut view| {
                    let trailers = get_fields_inner(view.get().table(), &trailers)?.clone();
                    anyhow::Ok(trailers)
                })?;
                let body = empty_body().with_trailers(async move { Some(Ok(trailers)) });
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
            } => {
                let body = empty_body();
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
            } => return Ok(Err(err)),
            Body::Host { .. } => bail!("host body is corrupted"),
            Body::Consumed => {
                let body = empty_body();
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
        };
        let (
            http::response::Parts {
                status, headers, ..
            },
            body,
        ) = response.into_parts();
        store.with(|mut view| {
            let body = Body::Host {
                stream: Some(body),
                buffer: None,
            };
            let response = Response::new(status, headers, body);
            let response = push_response(view.get().table(), response)?;
            Ok(Ok(response))
        })
    }
}

impl<T> handler::Host for WasiHttpImpl<T> where T: WasiHttpView {}
