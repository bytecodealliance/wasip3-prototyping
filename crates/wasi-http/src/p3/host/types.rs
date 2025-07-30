use crate::p3::bindings::http::types::{
    ErrorCode, FieldName, FieldValue, HeaderError, Host, HostFields, HostRequest,
    HostRequestOptions, HostRequestWithStore, HostResponse, HostResponseWithStore, Method,
    RequestOptionsError, Scheme, StatusCode, Trailers,
};
use crate::p3::host::{
    delete_fields, get_fields, get_fields_inner, get_fields_inner_mut, get_request,
    get_request_mut, get_response, get_response_mut, push_fields, push_fields_child, push_request,
    push_response,
};
use crate::p3::{
    Body, BodyContext, BodyFrame, ContentLength, DEFAULT_BUFFER_CAPACITY, MaybeTombstone, Request,
    RequestOptions, Response, WasiHttp, WasiHttpImpl, WasiHttpView,
};
use anyhow::{Context as _, bail};
use bytes::BytesMut;
use core::future::Future;
use core::future::poll_fn;
use core::mem;
use core::ops::{Deref, DerefMut};
use core::pin::{Pin, pin};
use core::str;
use core::task::Poll;
use futures::join;
use http::header::CONTENT_LENGTH;
use http_body::Body as _;
use std::io::Cursor;
use std::sync::Arc;
use wasmtime::component::{
    Accessor, AccessorTask, FutureReader, FutureWriter, GuardedFutureReader, GuardedFutureWriter,
    GuardedStreamReader, GuardedStreamWriter, Resource, StreamReader, StreamWriter,
};
use wasmtime_wasi::ResourceTable;
use wasmtime_wasi::p3::bindings::clocks::monotonic_clock::Duration;
use wasmtime_wasi::p3::{AbortOnDropHandle, ResourceView as _, WithChildren};

fn get_request_options<'a>(
    table: &'a ResourceTable,
    opts: &Resource<WithChildren<RequestOptions>>,
) -> wasmtime::Result<&'a WithChildren<RequestOptions>> {
    table
        .get(opts)
        .context("failed to get request options from table")
}

fn get_request_options_inner<'a>(
    table: &'a ResourceTable,
    opts: &Resource<WithChildren<RequestOptions>>,
) -> wasmtime::Result<impl Deref<Target = RequestOptions> + use<'a>> {
    let opts = get_request_options(table, opts)?;
    opts.get()
}

fn get_request_options_mut<'a>(
    table: &'a mut ResourceTable,
    opts: &Resource<WithChildren<RequestOptions>>,
) -> wasmtime::Result<&'a mut WithChildren<RequestOptions>> {
    table
        .get_mut(opts)
        .context("failed to get request options from table")
}

fn get_request_options_inner_mut<'a>(
    table: &'a mut ResourceTable,
    opts: &Resource<WithChildren<RequestOptions>>,
) -> wasmtime::Result<Option<impl DerefMut<Target = RequestOptions> + use<'a>>> {
    let opts = get_request_options_mut(table, opts)?;
    opts.get_mut()
}

fn push_request_options(
    table: &mut ResourceTable,
    fields: WithChildren<RequestOptions>,
) -> wasmtime::Result<Resource<WithChildren<RequestOptions>>> {
    table
        .push(fields)
        .context("failed to push request options to table")
}

fn delete_request_options(
    table: &mut ResourceTable,
    opts: Resource<WithChildren<RequestOptions>>,
) -> wasmtime::Result<WithChildren<RequestOptions>> {
    table
        .delete(opts)
        .context("failed to delete request options from table")
}

fn clone_trailer_result(
    res: &Result<Option<Resource<Trailers>>, ErrorCode>,
) -> Result<Option<Resource<Trailers>>, ErrorCode> {
    match res {
        Ok(None) => Ok(None),
        Ok(Some(trailers)) => Ok(Some(Resource::new_own(trailers.rep()))),
        Err(err) => Err(err.clone()),
    }
}

/// Extract the `Content-Length` header value from a [`http::HeaderMap`], returning `None` if it's not
/// present. This function will return `Err` if it's not possible to parse the `Content-Length`
/// header.
fn get_content_length(headers: &http::HeaderMap) -> wasmtime::Result<Option<u64>> {
    let Some(v) = headers.get(CONTENT_LENGTH) else {
        return Ok(None);
    };
    let v = v.to_str()?;
    let v = v.parse()?;
    Ok(Some(v))
}

type TrailerFuture = FutureReader<Result<Option<Resource<Trailers>>, ErrorCode>>;

struct BodyTask {
    cx: BodyContext,
    body: Arc<std::sync::Mutex<Body>>,
    contents_tx: StreamWriter<u8>,
    trailers_tx: FutureWriter<Result<Option<Resource<Trailers>>, ErrorCode>>,
}

impl<T, U> AccessorTask<T, WasiHttp<U>, wasmtime::Result<()>> for BodyTask
where
    U: WasiHttpView + 'static,
{
    async fn run(self, store: &Accessor<T, WasiHttp<U>>) -> wasmtime::Result<()> {
        let body = {
            let Ok(mut body) = self.body.lock() else {
                bail!("lock poisoned");
            };
            mem::replace(&mut *body, Body::Consumed)
        };
        let mut contents_tx = GuardedStreamWriter::new(store, self.contents_tx);
        let mut trailers_tx = GuardedFutureWriter::new(store, self.trailers_tx);
        match body {
            Body::Guest {
                contents: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(Ok(None))) | None,
                tx,
                content_length: Some(ContentLength { limit, sent }),
                ..
            } if limit != sent => {
                drop(contents_tx);
                join!(
                    async {
                        tx.write(store, Err(self.cx.as_body_size_error(sent))).await;
                    },
                    async {
                        trailers_tx
                            .write(Err(self.cx.as_body_size_error(sent)))
                            .await;
                    }
                );
                return Ok(());
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::Some(trailers_rx),
                buffer: None,
                tx,
                content_length: None,
            } => {
                drop(contents_tx);
                let res = {
                    let mut watch_reader = pin!(trailers_tx.watch_reader());
                    let mut trailers_rx = pin!(trailers_rx.read(store));
                    poll_fn(|cx| match watch_reader.as_mut().poll(cx) {
                        Poll::Ready(()) => return Poll::Ready(None),
                        Poll::Pending => trailers_rx.as_mut().poll(cx).map(Some),
                    })
                    .await
                };
                let Some(Some(res)) = res else {
                    let Ok(mut body) = self.body.lock() else {
                        bail!("lock poisoned");
                    };
                    *body = Body::Guest {
                        contents: MaybeTombstone::None,
                        // FIXME: implement future read cancellation and put the
                        // trailers back in here.
                        trailers: MaybeTombstone::Tombstone,
                        buffer: None,
                        tx,
                        content_length: None,
                    };
                    return Ok(());
                };
                if !trailers_tx.write(clone_trailer_result(&res)).await {
                    let Ok(mut body) = self.body.lock() else {
                        bail!("lock poisoned");
                    };
                    *body = Body::Guest {
                        contents: MaybeTombstone::None,
                        trailers: MaybeTombstone::None,
                        buffer: Some(BodyFrame::Trailers(res)),
                        tx,
                        content_length: None,
                    };
                    return Ok(());
                }
                tx.write(store, Ok(())).await;
                Ok(())
            }
            Body::Guest {
                contents: MaybeTombstone::None,
                trailers: MaybeTombstone::None,
                buffer: Some(BodyFrame::Trailers(res)),
                tx,
                content_length: None,
            } => {
                drop(contents_tx);
                if !trailers_tx.write(clone_trailer_result(&res)).await {
                    let Ok(mut body) = self.body.lock() else {
                        bail!("lock poisoned");
                    };
                    *body = Body::Guest {
                        contents: MaybeTombstone::None,
                        trailers: MaybeTombstone::None,
                        buffer: Some(BodyFrame::Trailers(res)),
                        tx,
                        content_length: None,
                    };
                    return Ok(());
                }
                tx.write(store, Ok(())).await;
                Ok(())
            }
            Body::Guest {
                contents: MaybeTombstone::Some(contents_rx),
                trailers: MaybeTombstone::Some(trailers_rx),
                buffer,
                tx,
                mut content_length,
            } => {
                let mut contents_rx = GuardedStreamReader::new(store, contents_rx);
                let trailers_rx = GuardedFutureReader::new(store, trailers_rx);
                match buffer {
                    Some(BodyFrame::Data(buffer)) => {
                        let buffer = contents_tx.write_all(Cursor::new(buffer)).await;
                        if contents_tx.is_closed() {
                            let Ok(mut body) = self.body.lock() else {
                                bail!("lock poisoned");
                            };
                            let pos = buffer.position().try_into()?;
                            let buffer = buffer.into_inner().split_off(pos);
                            *body = Body::Guest {
                                contents: MaybeTombstone::Some(contents_rx.into()),
                                trailers: MaybeTombstone::Some(trailers_rx.into()),
                                buffer: Some(BodyFrame::Data(buffer)),
                                tx,
                                content_length,
                            };
                            return Ok(());
                        }
                    }
                    Some(BodyFrame::Trailers(..)) => bail!("corrupted guest body state"),
                    None => {}
                }
                let mut rx_buffer = BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY);
                loop {
                    let res = {
                        let mut contents_tx_drop = pin!(contents_tx.watch_reader());
                        let mut next_chunk = pin!(contents_rx.read(rx_buffer));
                        poll_fn(|cx| match contents_tx_drop.as_mut().poll(cx) {
                            Poll::Ready(()) => return Poll::Ready(None),
                            Poll::Pending => next_chunk.as_mut().poll(cx).map(Some),
                        })
                        .await
                    };
                    let Some(b) = res else {
                        // read handle dropped
                        let Ok(mut body) = self.body.lock() else {
                            bail!("lock poisoned");
                        };
                        *body = Body::Guest {
                            // FIXME: cancellation support should be added to
                            // reads in Wasmtime to fully support this to avoid
                            // needing `Taken` at all.
                            contents: MaybeTombstone::Tombstone,
                            trailers: MaybeTombstone::Some(trailers_rx.into()),
                            buffer: None,
                            tx,
                            content_length,
                        };
                        return Ok(());
                    };
                    rx_buffer = b;
                    let mut tx_tail = contents_tx;
                    if contents_rx.is_closed() {
                        debug_assert!(rx_buffer.is_empty());
                        if let Some(ContentLength { limit, sent }) = content_length {
                            if limit != sent {
                                drop(tx_tail);
                                join!(
                                    async {
                                        tx.write(store, Err(self.cx.as_body_size_error(sent)))
                                            .await;
                                    },
                                    async {
                                        trailers_tx
                                            .write(Err(self.cx.as_body_size_error(sent)))
                                            .await;
                                    }
                                );
                                return Ok(());
                            }
                        }
                        break;
                    };
                    if let Some(ContentLength { limit, sent }) = &mut content_length {
                        let n = rx_buffer.len().try_into().ok();
                        let n = n.and_then(|n| sent.checked_add(n));
                        if let Err(n) = n
                            .map(|n| {
                                if n > *limit {
                                    Err(n)
                                } else {
                                    *sent = n;
                                    Ok(())
                                }
                            })
                            .unwrap_or(Err(u64::MAX))
                        {
                            drop(tx_tail);
                            join!(
                                async {
                                    tx.write(store, Err(self.cx.as_body_size_error(n))).await;
                                },
                                async {
                                    trailers_tx.write(Err(self.cx.as_body_size_error(n))).await;
                                }
                            );
                            return Ok(());
                        }
                    }
                    let buffer = rx_buffer.split().freeze();
                    rx_buffer.reserve(DEFAULT_BUFFER_CAPACITY);
                    let buffer = tx_tail.write_all(Cursor::new(buffer)).await;
                    if tx_tail.is_closed() {
                        let Ok(mut body) = self.body.lock() else {
                            bail!("lock poisoned");
                        };
                        let pos = buffer.position().try_into()?;
                        let buffer = buffer.into_inner().split_off(pos);
                        *body = Body::Guest {
                            contents: MaybeTombstone::Some(contents_rx.into()),
                            trailers: MaybeTombstone::Some(trailers_rx.into()),
                            buffer: Some(BodyFrame::Data(buffer)),
                            tx,
                            content_length,
                        };
                        return Ok(());
                    };
                    contents_tx = tx_tail;
                }

                let res = {
                    let mut watch_reader = pin!(trailers_tx.watch_reader());
                    let mut trailers_rx = pin!(trailers_rx.read());
                    poll_fn(|cx| match watch_reader.as_mut().poll(cx) {
                        Poll::Ready(()) => return Poll::Ready(None),
                        Poll::Pending => trailers_rx.as_mut().poll(cx).map(Some),
                    })
                    .await
                };
                let Some(Some(res)) = res else {
                    let Ok(mut body) = self.body.lock() else {
                        bail!("lock poisoned");
                    };
                    *body = Body::Guest {
                        contents: MaybeTombstone::None,
                        // FIXME: implement future read cancellation and put the
                        // trailers back in here.
                        trailers: MaybeTombstone::Tombstone,
                        buffer: None,
                        tx,
                        content_length: None,
                    };
                    return Ok(());
                };
                if !trailers_tx.write(clone_trailer_result(&res)).await {
                    let Ok(mut body) = self.body.lock() else {
                        bail!("lock poisoned");
                    };
                    *body = Body::Guest {
                        contents: MaybeTombstone::None,
                        trailers: MaybeTombstone::None,
                        buffer: Some(BodyFrame::Trailers(res)),
                        tx,
                        content_length: None,
                    };
                    return Ok(());
                }
                tx.write(store, Ok(())).await;
                Ok(())
            }
            Body::Guest { .. } => bail!("corrupted guest body state"),
            Body::Host {
                stream: Some(mut stream),
                buffer,
            } => {
                match buffer {
                    Some(BodyFrame::Data(buffer)) => {
                        let buffer = contents_tx.write_all(Cursor::new(buffer)).await;
                        if contents_tx.is_closed() {
                            let Ok(mut body) = self.body.lock() else {
                                bail!("lock poisoned");
                            };
                            let pos = buffer.position().try_into()?;
                            let buffer = buffer.into_inner().split_off(pos);
                            *body = Body::Host {
                                stream: Some(stream),
                                buffer: Some(BodyFrame::Data(buffer)),
                            };
                            return Ok(());
                        }
                    }
                    Some(BodyFrame::Trailers(..)) => bail!("corrupted guest body state"),
                    None => {}
                }
                loop {
                    let res = {
                        let mut watch_reader = pin!(contents_tx.watch_reader());
                        poll_fn(|cx| match watch_reader.as_mut().poll(cx) {
                            Poll::Ready(()) => return Poll::Ready(None),
                            Poll::Pending => Pin::new(&mut stream).poll_frame(cx).map(Some),
                        })
                        .await
                    };
                    match res {
                        None => {
                            // read handle dropped
                            let Ok(mut body) = self.body.lock() else {
                                bail!("lock poisoned");
                            };
                            *body = Body::Host {
                                stream: Some(stream),
                                buffer: None,
                            };
                            return Ok(());
                        }
                        Some(None) => {
                            drop(contents_tx);
                            if !trailers_tx.write(Ok(None)).await {
                                let Ok(mut body) = self.body.lock() else {
                                    bail!("lock poisoned");
                                };
                                *body = Body::Host {
                                    stream: None,
                                    buffer: Some(BodyFrame::Trailers(Ok(None))),
                                };
                            }
                            return Ok(());
                        }
                        Some(Some(Ok(frame))) => {
                            match frame.into_data().map_err(http_body::Frame::into_trailers) {
                                Ok(buffer) => {
                                    let buffer = contents_tx.write_all(Cursor::new(buffer)).await;
                                    if contents_tx.is_closed() {
                                        let Ok(mut body) = self.body.lock() else {
                                            bail!("lock poisoned");
                                        };
                                        let pos = buffer.position().try_into()?;
                                        let buffer = buffer.into_inner().split_off(pos);
                                        *body = Body::Host {
                                            stream: Some(stream),
                                            buffer: Some(BodyFrame::Data(buffer)),
                                        };
                                        return Ok(());
                                    };
                                }
                                Err(Ok(trailers)) => {
                                    drop(contents_tx);
                                    let trailers = store.with(|mut view| {
                                        push_fields(view.get().table(), WithChildren::new(trailers))
                                    })?;
                                    if !trailers_tx
                                        .write(Ok(Some(Resource::new_own(trailers.rep()))))
                                        .await
                                    {
                                        let Ok(mut body) = self.body.lock() else {
                                            bail!("lock poisoned");
                                        };
                                        *body = Body::Host {
                                            stream: None,
                                            buffer: Some(BodyFrame::Trailers(Ok(Some(trailers)))),
                                        };
                                    }
                                    return Ok(());
                                }
                                Err(Err(..)) => {
                                    drop(contents_tx);
                                    if !trailers_tx.write(Err(ErrorCode::HttpProtocolError)).await {
                                        let Ok(mut body) = self.body.lock() else {
                                            bail!("lock poisoned");
                                        };
                                        *body = Body::Host {
                                            stream: None,
                                            buffer: Some(BodyFrame::Trailers(Err(
                                                ErrorCode::HttpProtocolError,
                                            ))),
                                        };
                                    }
                                    return Ok(());
                                }
                            }
                        }
                        Some(Some(Err(err))) => {
                            drop(contents_tx);
                            if !trailers_tx.write(Err(err.clone())).await {
                                let Ok(mut body) = self.body.lock() else {
                                    bail!("lock poisoned");
                                };
                                *body = Body::Host {
                                    stream: None,
                                    buffer: Some(BodyFrame::Trailers(Err(err))),
                                };
                            }
                            return Ok(());
                        }
                    }
                }
            }
            Body::Host {
                stream: None,
                buffer: Some(BodyFrame::Trailers(res)),
            } => {
                drop(contents_tx);
                if !trailers_tx.write(clone_trailer_result(&res)).await {
                    let Ok(mut body) = self.body.lock() else {
                        bail!("lock poisoned");
                    };
                    *body = Body::Host {
                        stream: None,
                        buffer: Some(BodyFrame::Trailers(res)),
                    };
                }
                return Ok(());
            }
            Body::Host { .. } => bail!("corrupted host body state"),
            Body::Consumed => bail!("body is consumed"),
        }
    }
}

impl<T> Host for WasiHttpImpl<T> where T: WasiHttpView {}

fn parse_header_value(
    name: &http::HeaderName,
    value: impl AsRef<[u8]>,
) -> Result<http::HeaderValue, HeaderError> {
    if name == CONTENT_LENGTH {
        let s = str::from_utf8(value.as_ref()).or(Err(HeaderError::InvalidSyntax))?;
        let v: u64 = s.parse().or(Err(HeaderError::InvalidSyntax))?;
        Ok(v.into())
    } else {
        http::header::HeaderValue::from_bytes(value.as_ref()).or(Err(HeaderError::InvalidSyntax))
    }
}

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

        for (name, value) in entries {
            let Ok(name) = name.parse() else {
                return Ok(Err(HeaderError::InvalidSyntax));
            };
            if self.is_forbidden_header(&name) {
                return Ok(Err(HeaderError::Forbidden));
            }
            match parse_header_value(&name, value) {
                Ok(value) => {
                    fields.append(name, value);
                }
                Err(err) => return Ok(Err(err)),
            }
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
        if self.is_forbidden_header(&name) {
            return Ok(Err(HeaderError::Forbidden));
        }
        let mut values = Vec::with_capacity(value.len());
        for value in value {
            match parse_header_value(&name, value) {
                Ok(value) => {
                    values.push(value);
                }
                Err(err) => return Ok(Err(err)),
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
        if self.is_forbidden_header(&header) {
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
        if self.is_forbidden_header(&header) {
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
        let Ok(name) = name.parse() else {
            return Ok(Err(HeaderError::InvalidSyntax));
        };
        if self.is_forbidden_header(&name) {
            return Ok(Err(HeaderError::Forbidden));
        }
        let value = match parse_header_value(&name, value) {
            Ok(value) => value,
            Err(err) => return Ok(Err(err)),
        };
        let Some(mut fields) = get_fields_inner_mut(self.table(), &fields)? else {
            return Ok(Err(HeaderError::Immutable));
        };
        fields.append(name, value);
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

impl<T> HostRequestWithStore for WasiHttp<T>
where
    T: WasiHttpView + 'static,
{
    async fn new<U: 'static>(
        store: &Accessor<U, Self>,
        headers: Resource<WithChildren<http::HeaderMap>>,
        contents: Option<StreamReader<u8>>,
        trailers: TrailerFuture,
        options: Option<Resource<WithChildren<RequestOptions>>>,
    ) -> wasmtime::Result<(Resource<Request>, FutureReader<Result<(), ErrorCode>>)> {
        store.with(|mut view| {
            let instance = view.instance();
            let (res_tx, res_rx) = instance
                .future(&mut view, || Ok(()))
                .context("failed to create future")?;
            let contents = match contents {
                Some(contents) => MaybeTombstone::Some(contents),
                None => MaybeTombstone::None,
            };
            let mut binding = view.get();
            let table = binding.table();
            let headers = delete_fields(table, headers)?;
            let headers = headers.unwrap_or_clone()?;
            let content_length = get_content_length(&headers)?;
            let options = options
                .map(|options| {
                    let options = delete_request_options(table, options)?;
                    options.unwrap_or_clone()
                })
                .transpose()?;
            let body = Body::Guest {
                contents,
                trailers: MaybeTombstone::Some(trailers),
                buffer: None,
                tx: res_tx,
                content_length: content_length.map(ContentLength::new),
            };
            let req = push_request(
                table,
                Request::new(http::Method::GET, None, None, None, headers, body, options),
            )?;
            Ok((req, res_rx))
        })
    }

    async fn body<U: 'static>(
        store: &Accessor<U, Self>,
        req: Resource<Request>,
    ) -> wasmtime::Result<Result<(StreamReader<u8>, TrailerFuture), ()>> {
        store.with(|mut view| {
            let instance = view.instance();
            let (contents_tx, contents_rx) = instance
                .stream(&mut view)
                .context("failed to create stream")?;
            let (trailers_tx, trailers_rx) = instance
                .future(&mut view, || Ok(None))
                .context("failed to create future")?;
            let mut binding = view.get();
            let Request { body, .. } = get_request_mut(binding.table(), &req)?;
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
                cx: BodyContext::Request,
                body,
                contents_tx,
                trailers_tx,
            });
            let mut binding = view.get();
            let req = get_request_mut(binding.table(), &req)?;
            req.task = Some(AbortOnDropHandle(task));
            Ok(Ok((contents_rx, trailers_rx)))
        })
    }

    async fn drop<U: 'static>(
        store: &Accessor<U, Self>,
        req: Resource<Request>,
    ) -> wasmtime::Result<()> {
        store.with(|mut store| {
            let request = store
                .get()
                .table()
                .delete(req)
                .context("failed to delete request from table")?;
            mem::replace(request.body.lock().unwrap().deref_mut(), Body::Consumed)
                .drop_with_store(store);
            Ok(())
        })
    }
}

impl<T> HostRequest for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
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
        let has_port = authority.contains(':');
        let Ok(authority) = http::uri::Authority::try_from(authority) else {
            return Ok(Err(()));
        };
        if has_port && authority.port_u16().is_none() {
            return Ok(Err(()));
        }
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

impl<T> HostResponseWithStore for WasiHttp<T>
where
    T: WasiHttpView + 'static,
{
    async fn new<U: 'static>(
        store: &Accessor<U, Self>,
        headers: Resource<WithChildren<http::HeaderMap>>,
        contents: Option<StreamReader<u8>>,
        trailers: TrailerFuture,
    ) -> wasmtime::Result<(Resource<Response>, FutureReader<Result<(), ErrorCode>>)> {
        store.with(|mut view| {
            let instance = view.instance();
            let (res_tx, res_rx) = instance
                .future(&mut view, || Ok(()))
                .context("failed to create future")?;
            let contents = match contents {
                Some(contents) => MaybeTombstone::Some(contents),
                None => MaybeTombstone::None,
            };
            let mut binding = view.get();
            let table = binding.table();
            let headers = delete_fields(table, headers)?;
            let headers = headers.unwrap_or_clone()?;
            let content_length = get_content_length(&headers)?;
            let body = Body::Guest {
                contents,
                trailers: MaybeTombstone::Some(trailers),
                buffer: None,
                tx: res_tx,
                content_length: content_length.map(ContentLength::new),
            };
            let res = push_response(table, Response::new(http::StatusCode::OK, headers, body))?;
            Ok((res, res_rx))
        })
    }

    async fn body<U: 'static>(
        store: &Accessor<U, Self>,
        res: Resource<Response>,
    ) -> wasmtime::Result<Result<(StreamReader<u8>, TrailerFuture), ()>> {
        store.with(|mut view| {
            let instance = view.instance();
            let (contents_tx, contents_rx) = instance
                .stream(&mut view)
                .context("failed to create stream")?;
            let (trailers_tx, trailers_rx) = instance
                .future(&mut view, || Ok(None))
                .context("failed to create future")?;
            let mut binding = view.get();
            let Response { body, .. } = get_response_mut(binding.table(), &res)?;
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
                cx: BodyContext::Response,
                body,
                contents_tx,
                trailers_tx,
            });
            let mut binding = view.get();
            let res = get_response_mut(binding.table(), &res)?;
            res.body_task = Some(AbortOnDropHandle(task));
            Ok(Ok((contents_rx, trailers_rx)))
        })
    }

    async fn drop<U: 'static>(
        store: &Accessor<U, Self>,
        req: Resource<Response>,
    ) -> wasmtime::Result<()> {
        store.with(|mut store| {
            let request = store
                .get()
                .table()
                .delete(req)
                .context("failed to delete request from table")?;
            mem::replace(request.body.lock().unwrap().deref_mut(), Body::Consumed)
                .drop_with_store(store);
            Ok(())
        })
    }
}
impl<T> HostResponse for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
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
}
