use core::iter;

use std::sync::Arc;

use anyhow::bail;
use bytes::Bytes;
use futures::StreamExt as _;
use http::header::HOST;
use http::{HeaderValue, Uri};
use http_body_util::{BodyExt as _, BodyStream, StreamBody};
use tokio::sync::oneshot;
use wasmtime::component::{Accessor, AccessorTask, Resource};
use wasmtime_wasi::p3::{AccessorTaskFn, ResourceView as _};

use crate::p3::bindings::http::handler;
use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{
    empty_body, Body, BodyFrame, Client as _, GuestBody, GuestRequestTrailers,
    OutgoingTrailerFuture, Request, Response, WasiHttpImpl, WasiHttpView,
};

use super::{delete_request, get_fields_inner, push_response};

struct TrailerTask {
    rx: OutgoingTrailerFuture,
    tx: oneshot::Sender<Result<Option<http::HeaderMap>, ErrorCode>>,
}

impl<T, U: WasiHttpView> AccessorTask<T, U, wasmtime::Result<()>> for TrailerTask {
    async fn run(self, store: &mut Accessor<T, U>) -> wasmtime::Result<()> {
        match self.rx.await {
            Ok(Ok(trailers)) => store.with(|mut view| {
                let trailers = trailers
                    .map(|trailers| get_fields_inner(view.table(), &trailers))
                    .transpose()?;
                _ = self.tx.send(Ok(trailers.as_deref().cloned()));
                Ok(())
            }),
            Ok(Err(err)) => {
                _ = self.tx.send(Err(err));
                Ok(())
            }
            Err(..) => Ok(()),
        }
    }
}

impl<T> handler::Host for WasiHttpImpl<&mut T>
where
    T: WasiHttpView + 'static,
{
    async fn handle<U: 'static>(
        store: &mut Accessor<U, Self>,
        request: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<Response>, ErrorCode>> {
        eprintln!("[host] call handle");
        let Request {
            method,
            scheme,
            authority,
            path_with_query,
            headers,
            body,
            options,
            ..
        } = store.with(|mut view| delete_request(view.table(), request))?;

        let mut client = store.with(|view| view.http().client.clone());

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
        let Ok(uri) = uri.build() else {
            return Ok(Err(ErrorCode::HttpRequestUriInvalid));
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
        eprintln!("[host] have built req {request:?}");
        let (response, task) = match body {
            Body::Guest {
                contents: None,
                trailers: None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
                tx,
            } => {
                let body = empty_body();
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            let res = io.await;
                            tx.write(res.map_err(Into::into)).into_future().await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
            } => {
                let trailers = store.with(|mut view| {
                    let trailers = get_fields_inner(view.table(), &trailers)?;
                    anyhow::Ok(trailers.clone())
                })?;
                let body = empty_body().with_trailers(async move { Some(Ok(trailers)) });
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            let res = io.await;
                            tx.write(res.map_err(Into::into)).into_future().await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
                tx: _,
            } => return Ok(Err(err)),
            Body::Guest {
                contents: None,
                trailers: Some(trailers),
                buffer: None,
                tx,
            } => {
                eprintln!("[host] no contents, only trailers");
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let task = store.spawn(TrailerTask {
                    rx: trailers,
                    tx: trailers_tx,
                });
                let body = empty_body().with_trailers(GuestRequestTrailers {
                    trailers: Some(trailers_rx),
                    trailer_task: task.abort_handle(),
                });
                let request = request.map(|()| body);
                eprintln!("[host] send request..");
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            eprintln!("[host] await Tx result..");
                            let res = io.await;
                            eprintln!("[host] write Tx result..");
                            tx.write(res.map_err(Into::into)).into_future().await;
                            eprintln!("[host] done writing Tx result");
                            Ok(())
                        }));
                        eprintln!("[host] return Ok response");
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
            } => {
                eprintln!("[host] contents, trailers");
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let task = store.spawn(TrailerTask {
                    rx: trailers,
                    tx: trailers_tx,
                });
                let buffer = match buffer {
                    Some(BodyFrame::Data(buf)) => buf,
                    Some(BodyFrame::Trailers(..)) => bail!("guest body is corrupted"),
                    None => Bytes::default(),
                };
                let body = GuestBody::new(contents, buffer).with_trailers(GuestRequestTrailers {
                    trailers: Some(trailers_rx),
                    trailer_task: task.abort_handle(),
                });
                let request = request.map(|()| body);
                eprintln!("[host] send request..");
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            eprintln!("[host] await Tx result..");
                            let res = io.await;
                            eprintln!("[host] write Tx result..");
                            tx.write(res.map_err(Into::into)).into_future().await;
                            eprintln!("[host] done writing Tx result");
                            Ok(())
                        }));
                        eprintln!("[host] return Ok response");
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
                    let trailers = get_fields_inner(view.table(), &trailers)?;
                    anyhow::Ok(trailers.clone())
                })?;
                let body = empty_body().with_trailers(async move { Some(Ok(trailers)) });
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
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
                let request = request.map(|()| body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        let task = store.spawn(AccessorTaskFn(|_: &mut Accessor<U, Self>| async {
                            _ = io.await;
                            Ok(())
                        }));
                        let task = task.abort_handle();
                        match response.await {
                            Ok(response) => {
                                (response.map(|body| body.map_err(Into::into).boxed()), task)
                            }
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
        };
        eprintln!("[host] handle return");
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
            let response = Response::new_incoming(status, headers, body, task);
            let response = push_response(view.table(), response)?;
            Ok(Ok(response))
        })
    }
}
