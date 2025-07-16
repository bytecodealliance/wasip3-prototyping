use super::{delete_request, get_fields_inner, push_response};
use crate::p3::bindings::http::handler;
use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{
    Body, BodyChannel, BodyFrame, BodyWithContentLength, Client as _, ContentLength,
    DEFAULT_BUFFER_CAPACITY, MaybeTombstone, Request, Response, WasiHttp, WasiHttpImpl,
    WasiHttpView, empty_body, handle_guest_trailers,
};
use anyhow::bail;
use bytes::{Bytes, BytesMut};
use core::iter;
use futures::StreamExt as _;
use http::header::HOST;
use http::{HeaderValue, Uri};
use http_body_util::{BodyExt as _, BodyStream, StreamBody};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use wasmtime::component::{Accessor, Resource};
use wasmtime_wasi::p3::{AbortOnDropHandle, ResourceView as _, SpawnExt};

impl<T> handler::HostConcurrent for WasiHttp<T>
where
    T: WasiHttpView + 'static,
{
    async fn handle<U: 'static>(
        store: &Accessor<U, Self>,
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
                contents: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(Ok(None))) | None,
                tx,
                content_length: Some(ContentLength { limit, sent }),
                ..
            } if limit != sent => {
                store.spawn_fn_box(move |store: &Accessor<U, Self>| {
                    Box::pin(async move {
                        tx.write(store, Err(ErrorCode::HttpRequestBodySize(Some(sent))))
                            .await;
                        Ok(())
                    })
                });
                return Ok(Err(ErrorCode::HttpRequestBodySize(Some(sent))));
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(Ok(None))),
                tx,
                content_length: None,
            } => {
                let body = empty_body();
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn_fn_box(move |store| {
                            Box::pin(async move {
                                let res = io.await;
                                tx.write(store, res.map_err(Into::into)).await;
                                Ok(())
                            })
                        });
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
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
                        store.spawn_fn_box(move |store| {
                            Box::pin(async move {
                                let res = io.await;
                                tx.write(store, res.map_err(Into::into)).await;
                                Ok(())
                            })
                        });
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(Err(err))),
                tx,
                content_length: None,
            } => {
                store.spawn_fn_box({
                    let err = err.clone();
                    move |store| {
                        Box::pin(async move {
                            tx.write(store, Err(err)).await;
                            Ok(())
                        })
                    }
                });
                return Ok(Err(err));
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::Some(trailers),
                buffer: None,
                tx,
                content_length: None,
            } => {
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let task = AbortOnDropHandle(store.spawn_fn_box(|store| {
                    Box::pin(handle_guest_trailers(store, trailers, trailers_tx))
                }));
                let body = empty_body().with_trailers(async {
                    let result = trailers_rx.await.ok();
                    drop(task);
                    result
                });
                let request = http::Request::from_parts(request, body);
                match client.send_request(request, options).await? {
                    Ok((response, io)) => {
                        store.spawn_fn_box(|store| {
                            Box::pin(async move {
                                let res = io.await;
                                tx.write(store, res.map_err(Into::into)).await;
                                Ok(())
                            })
                        });
                        match response.await {
                            Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
                            Err(err) => return Ok(Err(err)),
                        }
                    }
                    Err(err) => return Ok(Err(err)),
                }
            }
            Body::Guest {
                contents: MaybeTombstone::Some(mut contents),
                trailers: MaybeTombstone::Some(trailers),
                buffer,
                tx,
                content_length,
            } => {
                let (trailers_tx, trailers_rx) = oneshot::channel();
                let (body_tx, body_rx) = mpsc::channel(1);
                let task = AbortOnDropHandle(store.spawn_fn_box(|store| {
                    Box::pin(handle_guest_trailers(store, trailers, trailers_tx))
                }));
                let buffer = match buffer {
                    Some(BodyFrame::Data(buffer)) => buffer,
                    Some(BodyFrame::Trailers(..)) => bail!("guest body is corrupted"),
                    None => Bytes::default(),
                };
                let body = BodyChannel::new(body_rx);
                let body =
                    BodyWithContentLength::new(body, content_length).with_trailers(async move {
                        let result = trailers_rx.await.ok();
                        drop(task);
                        result
                    });
                let request = http::Request::from_parts(request, body);
                let (response, io) = match client.send_request(request, options).await? {
                    Ok(pair) => pair,
                    Err(err) => return Ok(Err(err)),
                };
                store.spawn_fn_box(move |store| Box::pin(async move {
                    let (io_res, body_res) = futures::join! {
                        io,
                        async {
                            body_tx.send(Ok(buffer)).await?;
                            let mut rx_buffer = BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY);
                            while !contents.is_closed() {
                                rx_buffer = contents.read(store,rx_buffer).await;
                                let buffer = rx_buffer.split();
                                body_tx.send(Ok(buffer.freeze())).await?;
                                rx_buffer.reserve(DEFAULT_BUFFER_CAPACITY);
                            }
                            drop(body_tx);
                            anyhow::Ok(())
                        }
                    };
                    // Failure in sending the body only happens when the body
                    // itself goes away due to cancellation elsewhere, so
                    // swallow this error.
                    let _ = body_res;
                    tx.write(store,io_res.map_err(Into::into)).await;
                    Ok(())
                }));
                match response.await {
                    Ok(response) => response.map(|body| body.map_err(Into::into).boxed()),
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
                        store.spawn_fn(|_| async {
                            _ = io.await;
                            Ok(())
                        });
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
                        store.spawn_fn(|_| async {
                            _ = io.await;
                            Ok(())
                        });
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
                        store.spawn_fn(|_| async {
                            _ = io.await;
                            Ok(())
                        });
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
                        store.spawn_fn(|_| async {
                            _ = io.await;
                            Ok(())
                        });
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
                        store.spawn_fn(|_| async {
                            _ = io.await;
                            Ok(())
                        });
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
