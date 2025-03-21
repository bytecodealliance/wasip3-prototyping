use core::future::{poll_fn, Future as _};
use core::mem;
use core::pin::Pin;
use core::task::Poll;

use std::sync::Arc;

use anyhow::{bail, Context as _};
use bytes::Bytes;
use http_body::Body as _;
use wasmtime::component::{
    future, stream, Accessor, AccessorTask, FutureWriter, HostFuture, HostStream, Resource,
    StreamWriter,
};
use wasmtime_wasi::p3::bindings::clocks::monotonic_clock::Duration;
use wasmtime_wasi::p3::{ResourceView as _, WithChildren};

use crate::p3::bindings::http::types::{
    ErrorCode, FieldName, FieldValue, HeaderError, Host, HostFields, HostRequest,
    HostRequestOptions, HostResponse, Method, RequestOptionsError, Scheme, StatusCode, Trailers,
};
use crate::p3::host::{
    delete_fields, delete_request_options, delete_response, get_fields, get_fields_inner,
    get_fields_inner_mut, get_request, get_request_mut, get_request_options_inner,
    get_request_options_inner_mut, get_response, get_response_mut, is_forbidden_header,
    push_fields, push_fields_child, push_request, push_request_options, push_response,
};
use crate::p3::{Body, Request, RequestOptions, Response, WasiHttpImpl, WasiHttpView};

use super::get_request_options;

type TrailerFuture = HostFuture<Result<Option<Resource<Trailers>>, ErrorCode>>;

struct BodyTask {
    body: Arc<std::sync::Mutex<Body>>,
    contents_tx: StreamWriter<Bytes>,
    trailers_tx: FutureWriter<Result<Option<Resource<Trailers>>, ErrorCode>>,
}

impl<T, U> AccessorTask<T, U, wasmtime::Result<()>> for BodyTask {
    async fn run(self, store: &mut Accessor<T, U>) -> wasmtime::Result<()> {
        let body = {
            let Ok(mut body) = self.body.lock() else {
                bail!("lock poisoned");
            };
            mem::replace(&mut *body, Body::Consumed)
        };
        let mut contents_tx = self.contents_tx.watch_reader();
        match body {
            Body::Outgoing {
                contents: mut contents_rx,
                trailers: trailers_rx,
                tx,
            } => {
                loop {
                    let Some(rx) = poll_fn(|cx| match Pin::new(&mut contents_tx).poll(cx) {
                        Poll::Ready(()) => return Poll::Ready(None),
                        Poll::Pending => contents_rx.as_mut().poll(cx).map(Some),
                    })
                    .await
                    else {
                        // read handle dropped
                        let Ok(mut body) = self.body.lock() else {
                            bail!("lock poisoned");
                        };
                        *body = Body::Outgoing {
                            contents: contents_rx,
                            trailers: trailers_rx,
                            tx,
                        };
                        return Ok(());
                    };
                    let Ok((rx_tail, buf)) = rx else {
                        break;
                    };
                    contents_rx = rx_tail.read().into_future();
                    let tx_tail = contents_tx.into_inner().await;
                    let Some(tx_tail) = tx_tail.write(buf).into_future().await else {
                        let Ok(mut body) = self.body.lock() else {
                            bail!("lock poisoned");
                        };
                        *body = Body::Outgoing {
                            contents: contents_rx,
                            trailers: trailers_rx,
                            tx,
                        };
                        return Ok(());
                    };
                    contents_tx = tx_tail.watch_reader();
                }

                let mut trailers_tx = self.trailers_tx.watch_reader();
                let trailers_rx = store.with(|view| trailers_rx.into_reader(view));
                let mut trailers_rx = trailers_rx.read().into_future();
                let Some(Ok(res)) = poll_fn(|cx| match Pin::new(&mut trailers_tx).poll(cx) {
                    Poll::Ready(()) => return Poll::Ready(None),
                    Poll::Pending => trailers_rx.as_mut().poll(cx).map(Some),
                })
                .await
                else {
                    return Ok(());
                };
                let trailers_tx = trailers_tx.into_inner().await;
                trailers_tx.write(res);
                Ok(())
            }
            Body::Incoming(mut stream) => {
                loop {
                    match poll_fn(|cx| match Pin::new(&mut contents_tx).poll(cx) {
                        Poll::Ready(()) => return Poll::Ready(None),
                        Poll::Pending => Pin::new(&mut stream).poll_frame(cx).map(Some),
                    })
                    .await
                    {
                        None => {
                            // read handle dropped
                            let Ok(mut body) = self.body.lock() else {
                                bail!("lock poisoned");
                            };
                            *body = Body::Incoming(stream);
                            return Ok(());
                        }
                        Some(None) => {
                            self.trailers_tx.write(Ok(None));
                            return Ok(());
                        }
                        Some(Some(Ok(frame))) => {
                            // TODO: Handle frame
                            //contents_tx = tx_tail.watch_reader();
                        }
                        Some(Some(Err(err))) => {
                            self.trailers_tx.write(Err(err));
                            return Ok(());
                        }
                    }
                }
            }
            Body::Consumed => bail!("body is consumed"),
        }
    }
}

impl<T> Host for WasiHttpImpl<T> where T: WasiHttpView {}

impl<T> HostFields for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(&mut self) -> wasmtime::Result<Resource<WithChildren<http::HeaderMap>>> {
        push_fields(self.table(), WithChildren::default())
    }

    fn from_list(
        &mut self,
        entries: Vec<(FieldName, FieldValue)>,
    ) -> wasmtime::Result<Result<Resource<WithChildren<http::HeaderMap>>, HeaderError>> {
        let mut fields = http::HeaderMap::new();

        for (header, value) in entries {
            let Ok(header) = header.parse() else {
                return Ok(Err(HeaderError::InvalidSyntax));
            };
            if is_forbidden_header(self, &header) {
                return Ok(Err(HeaderError::Forbidden));
            }
            let value = match http::header::HeaderValue::from_bytes(&value) {
                Ok(value) => value,
                Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
            };
            fields.append(header, value);
        }
        let fields = push_fields(self.table(), WithChildren::new(fields))?;
        Ok(Ok(fields))
    }

    fn get(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
        name: FieldName,
    ) -> wasmtime::Result<Vec<FieldValue>> {
        let fields = get_fields_inner(self.table(), &fields)?;
        Ok(fields
            .get_all(name)
            .into_iter()
            .map(|val| val.as_bytes().into())
            .collect())
    }

    fn has(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
        name: FieldName,
    ) -> wasmtime::Result<bool> {
        let fields = get_fields_inner(self.table(), &fields)?;
        Ok(fields.contains_key(name))
    }

    fn set(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
        name: FieldName,
        value: Vec<FieldValue>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let Ok(name) = name.parse() else {
            return Ok(Err(HeaderError::InvalidSyntax));
        };
        if is_forbidden_header(self, &name) {
            return Ok(Err(HeaderError::Forbidden));
        }
        let mut values = Vec::with_capacity(value.len());
        for value in value {
            match http::header::HeaderValue::from_bytes(&value) {
                Ok(value) => values.push(value),
                Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
            }
        }
        let Some(mut fields) = get_fields_inner_mut(self.table(), &fields)? else {
            return Ok(Err(HeaderError::Immutable));
        };
        fields.remove(&name);
        for value in values {
            fields.append(&name, value);
        }
        Ok(Ok(()))
    }

    fn delete(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
        name: FieldName,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let header = match http::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
        };
        if is_forbidden_header(self, &header) {
            return Ok(Err(HeaderError::Forbidden));
        }
        let Some(mut fields) = get_fields_inner_mut(self.table(), &fields)? else {
            return Ok(Err(HeaderError::Immutable));
        };
        fields.remove(&name);
        Ok(Ok(()))
    }

    fn get_and_delete(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
        name: FieldName,
    ) -> wasmtime::Result<Result<Vec<FieldValue>, HeaderError>> {
        let Ok(header) = http::header::HeaderName::from_bytes(name.as_bytes()) else {
            return Ok(Err(HeaderError::InvalidSyntax));
        };
        if is_forbidden_header(self, &header) {
            return Ok(Err(HeaderError::Forbidden));
        }
        let Some(mut fields) = get_fields_inner_mut(self.table(), &fields)? else {
            return Ok(Err(HeaderError::Immutable));
        };
        let http::header::Entry::Occupied(entry) = fields.entry(header) else {
            return Ok(Ok(vec![]));
        };
        let (.., values) = entry.remove_entry_mult();
        Ok(Ok(values.map(|header| header.as_bytes().into()).collect()))
    }

    fn append(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
        name: FieldName,
        value: FieldValue,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let header = match http::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
        };
        if is_forbidden_header(self, &header) {
            return Ok(Err(HeaderError::Forbidden));
        }
        let value = match http::header::HeaderValue::from_bytes(&value) {
            Ok(value) => value,
            Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
        };
        let Some(mut fields) = get_fields_inner_mut(self.table(), &fields)? else {
            return Ok(Err(HeaderError::Immutable));
        };
        fields.append(header, value);
        Ok(Ok(()))
    }

    fn entries(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
    ) -> wasmtime::Result<Vec<(FieldName, FieldValue)>> {
        let fields = get_fields_inner(self.table(), &fields)?;
        let fields = fields
            .iter()
            .map(|(name, value)| (name.as_str().into(), value.as_bytes().into()))
            .collect();
        Ok(fields)
    }

    fn clone(
        &mut self,
        fields: Resource<WithChildren<http::HeaderMap>>,
    ) -> wasmtime::Result<Resource<WithChildren<http::HeaderMap>>> {
        let table = self.table();
        let fields = get_fields(table, &fields)?;
        let fields = fields.clone()?;
        push_fields(table, fields)
    }

    fn drop(&mut self, fields: Resource<WithChildren<http::HeaderMap>>) -> wasmtime::Result<()> {
        delete_fields(self.table(), fields)?;
        Ok(())
    }
}

impl<T> HostRequest for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    async fn new<U>(
        store: &mut Accessor<U, Self>,
        headers: Resource<WithChildren<http::HeaderMap>>,
        contents: HostStream<u8>,
        trailers: TrailerFuture,
        options: Option<Resource<WithChildren<RequestOptions>>>,
    ) -> wasmtime::Result<(Resource<Request>, HostFuture<Result<(), ErrorCode>>)> {
        store.with(|mut view| {
            let (res_tx, res_rx) = future(&mut view).context("failed to create future")?;
            let contents = contents.into_reader(&mut view).read().into_future();
            let table = view.table();
            let headers = delete_fields(table, headers)?;
            let headers = headers.unwrap_or_clone()?;
            let options = options
                .map(|options| {
                    let options = delete_request_options(table, options)?;
                    options.unwrap_or_clone()
                })
                .transpose()?;
            let body = Body::Outgoing {
                contents,
                trailers,
                tx: res_tx,
            };
            let req = push_request(table, Request::new(headers, body, options))?;
            Ok((req, res_rx.into()))
        })
    }

    fn method(&mut self, req: Resource<Request>) -> wasmtime::Result<Method> {
        let Request { method, .. } = get_request(self.table(), &req)?;
        Ok(method.into())
    }

    fn set_method(
        &mut self,
        req: Resource<Request>,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = get_request_mut(self.table(), &req)?;
        let Ok(method) = method.try_into() else {
            return Ok(Err(()));
        };
        req.method = method;
        Ok(Ok(()))
    }

    fn path_with_query(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<String>> {
        let Request {
            path_with_query, ..
        } = get_request(self.table(), &req)?;
        Ok(path_with_query.as_ref().map(|pq| pq.as_str().into()))
    }

    fn set_path_with_query(
        &mut self,
        req: Resource<Request>,
        path_with_query: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = get_request_mut(self.table(), &req)?;
        let Some(path_with_query) = path_with_query else {
            req.path_with_query = None;
            return Ok(Ok(()));
        };
        let Ok(path_with_query) = path_with_query.try_into() else {
            return Ok(Err(()));
        };
        req.path_with_query = Some(path_with_query);
        Ok(Ok(()))
    }

    fn scheme(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<Scheme>> {
        let Request { scheme, .. } = get_request(self.table(), &req)?;
        Ok(scheme.as_ref().map(Into::into))
    }

    fn set_scheme(
        &mut self,
        req: Resource<Request>,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = get_request_mut(self.table(), &req)?;
        let Some(scheme) = scheme else {
            req.scheme = None;
            return Ok(Ok(()));
        };
        let Ok(scheme) = scheme.try_into() else {
            return Ok(Err(()));
        };
        req.scheme = Some(scheme);
        Ok(Ok(()))
    }

    fn authority(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<String>> {
        let Request { authority, .. } = get_request(self.table(), &req)?;
        Ok(authority.as_ref().map(|auth| auth.as_str().into()))
    }

    fn set_authority(
        &mut self,
        req: Resource<Request>,
        authority: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = get_request_mut(self.table(), &req)?;
        let Some(authority) = authority else {
            req.authority = None;
            return Ok(Ok(()));
        };
        let Ok(authority) = authority.try_into() else {
            return Ok(Err(()));
        };
        req.authority = Some(authority);
        Ok(Ok(()))
    }

    fn options(
        &mut self,
        req: Resource<Request>,
    ) -> wasmtime::Result<Option<Resource<WithChildren<RequestOptions>>>> {
        let table = self.table();
        let Request { options, .. } = get_request(table, &req)?;
        if let Some(options) = options {
            let options = push_request_options(table, options.child())?;
            Ok(Some(options))
        } else {
            Ok(None)
        }
    }

    fn headers(
        &mut self,
        req: Resource<Request>,
    ) -> wasmtime::Result<Resource<WithChildren<http::HeaderMap>>> {
        let table = self.table();
        let Request { headers, .. } = get_request(table, &req)?;
        push_fields_child(table, headers.child(), &req)
    }

    async fn body<U: 'static>(
        store: &mut Accessor<U, Self>,
        req: Resource<Request>,
    ) -> wasmtime::Result<Result<(HostStream<u8>, TrailerFuture), ()>> {
        store.with(|mut view| {
            let (contents_tx, contents_rx) =
                stream(&mut view).context("failed to create stream")?;
            let (trailers_tx, trailers_rx) =
                future(&mut view).context("failed to create future")?;
            let Request { body, .. } = get_request_mut(view.table(), &req)?;
            {
                let Some(body) = Arc::get_mut(body) else {
                    return Ok(Err(()));
                };
                let Ok(body) = body.get_mut() else {
                    bail!("lock poisoned");
                };
                if matches!(body, Body::Consumed) {
                    return Ok(Err(()));
                }
            }
            let body = Arc::clone(&body);
            let task = view.spawn(BodyTask {
                body,
                contents_tx,
                trailers_tx,
            });
            let req = get_request_mut(view.table(), &req)?;
            req.task = Some(task.abort_handle());
            Ok(Ok((contents_rx.into(), trailers_rx.into())))
        })
    }

    fn drop(&mut self, req: Resource<Request>) -> wasmtime::Result<()> {
        self.table()
            .delete(req)
            .context("failed to delete request from table")?;
        Ok(())
    }
}

impl<T> HostRequestOptions for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(&mut self) -> wasmtime::Result<Resource<WithChildren<RequestOptions>>> {
        push_request_options(self.table(), WithChildren::default())
    }

    fn connect_timeout(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
    ) -> wasmtime::Result<Option<Duration>> {
        let RequestOptions {
            connect_timeout: Some(connect_timeout),
            ..
        } = *get_request_options_inner(self.table(), &opts)?
        else {
            return Ok(None);
        };
        let ns = connect_timeout.as_nanos();
        let ns = ns
            .try_into()
            .context("connect timeout duration nanoseconds do not fit in u64")?;
        Ok(Some(ns))
    }

    fn set_connect_timeout(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
        duration: Option<Duration>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        let Some(mut opts) = get_request_options_inner_mut(self.table(), &opts)? else {
            return Ok(Err(RequestOptionsError::Immutable));
        };
        opts.connect_timeout = duration.map(core::time::Duration::from_nanos);
        Ok(Ok(()))
    }

    fn first_byte_timeout(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
    ) -> wasmtime::Result<Option<Duration>> {
        let RequestOptions {
            first_byte_timeout: Some(first_byte_timeout),
            ..
        } = *get_request_options_inner(self.table(), &opts)?
        else {
            return Ok(None);
        };
        let ns = first_byte_timeout.as_nanos();
        let ns = ns
            .try_into()
            .context("first byte timeout duration nanoseconds do not fit in u64")?;
        Ok(Some(ns))
    }

    fn set_first_byte_timeout(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
        duration: Option<Duration>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        let Some(mut opts) = get_request_options_inner_mut(self.table(), &opts)? else {
            return Ok(Err(RequestOptionsError::Immutable));
        };
        opts.first_byte_timeout = duration.map(core::time::Duration::from_nanos);
        Ok(Ok(()))
    }

    fn between_bytes_timeout(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
    ) -> wasmtime::Result<Option<Duration>> {
        let RequestOptions {
            between_bytes_timeout: Some(between_bytes_timeout),
            ..
        } = *get_request_options_inner(self.table(), &opts)?
        else {
            return Ok(None);
        };
        let ns = between_bytes_timeout.as_nanos();
        let ns = ns
            .try_into()
            .context("between bytes timeout duration nanoseconds do not fit in u64")?;
        Ok(Some(ns))
    }

    fn set_between_bytes_timeout(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
        duration: Option<Duration>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        let Some(mut opts) = get_request_options_inner_mut(self.table(), &opts)? else {
            return Ok(Err(RequestOptionsError::Immutable));
        };
        opts.between_bytes_timeout = duration.map(core::time::Duration::from_nanos);
        Ok(Ok(()))
    }

    fn drop(&mut self, opts: Resource<WithChildren<RequestOptions>>) -> wasmtime::Result<()> {
        delete_request_options(self.table(), opts)?;
        Ok(())
    }

    fn clone(
        &mut self,
        opts: Resource<WithChildren<RequestOptions>>,
    ) -> wasmtime::Result<Resource<WithChildren<RequestOptions>>> {
        let table = self.table();
        let opts = get_request_options(table, &opts)?;
        let opts = opts.clone()?;
        push_request_options(table, opts)
    }
}

impl<T> HostResponse for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    async fn new<U>(
        store: &mut Accessor<U, Self>,
        headers: Resource<WithChildren<http::HeaderMap>>,
        contents: HostStream<u8>,
        trailers: TrailerFuture,
    ) -> wasmtime::Result<(Resource<Response>, HostFuture<Result<(), ErrorCode>>)> {
        store.with(|mut view| {
            let (res_tx, res_rx) = future(&mut view).context("failed to create future")?;
            let contents = contents.into_reader(&mut view).read().into_future();
            let table = view.table();
            let headers = delete_fields(table, headers)?;
            let headers = headers.unwrap_or_clone()?;
            let body = Body::Outgoing {
                contents,
                trailers,
                tx: res_tx,
            };
            let res = push_response(table, Response::new(headers, body))?;
            Ok((res, res_rx.into()))
        })
    }

    fn status_code(&mut self, res: Resource<Response>) -> wasmtime::Result<StatusCode> {
        let res = get_response(self.table(), &res)?;
        Ok(res.status.into())
    }

    fn set_status_code(
        &mut self,
        res: Resource<Response>,
        status_code: StatusCode,
    ) -> wasmtime::Result<Result<(), ()>> {
        let res = get_response_mut(self.table(), &res)?;
        let Ok(status) = http::StatusCode::from_u16(status_code) else {
            return Ok(Err(()));
        };
        res.status = status;
        Ok(Ok(()))
    }

    fn headers(
        &mut self,
        res: Resource<Response>,
    ) -> wasmtime::Result<Resource<WithChildren<http::HeaderMap>>> {
        let table = self.table();
        let Response { headers, .. } = get_response(table, &res)?;
        push_fields_child(table, headers.child(), &res)
    }

    async fn body<U: 'static>(
        store: &mut Accessor<U, Self>,
        res: Resource<Response>,
    ) -> wasmtime::Result<Result<(HostStream<u8>, TrailerFuture), ()>> {
        store.with(|mut view| {
            let (contents_tx, contents_rx) =
                stream(&mut view).context("failed to create stream")?;
            let (trailers_tx, trailers_rx) =
                future(&mut view).context("failed to create future")?;
            let Response { body, .. } = get_response_mut(view.table(), &res)?;
            {
                let Some(body) = Arc::get_mut(body) else {
                    return Ok(Err(()));
                };
                let Ok(body) = body.get_mut() else {
                    bail!("lock poisoned");
                };
                if matches!(body, Body::Consumed) {
                    return Ok(Err(()));
                }
            }
            let body = Arc::clone(&body);
            let task = view.spawn(BodyTask {
                body,
                contents_tx,
                trailers_tx,
            });
            let res = get_response_mut(view.table(), &res)?;
            res.task = Some(task.abort_handle());
            Ok(Ok((contents_rx.into(), trailers_rx.into())))
        })
    }

    fn drop(&mut self, res: Resource<Response>) -> wasmtime::Result<()> {
        delete_response(self.table(), res)?;
        Ok(())
    }
}
