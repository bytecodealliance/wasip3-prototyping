#![allow(unused)]

use anyhow::{anyhow, Context as _};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use wasmtime::component::{
    stream, Accessor, AccessorTask, FutureReader, HostFuture, HostStream, Resource, StreamReader,
    StreamWriter,
};
use wasmtime_wasi::p3::bindings::clocks::monotonic_clock::Duration;
use wasmtime_wasi::p3::ResourceView as _;
use wasmtime_wasi::ResourceTable;

use crate::p3::bindings::http::types::{
    Body, ErrorCode, FieldName, FieldValue, HeaderError, Headers, Method, Request, RequestOptions,
    RequestOptionsError, Response, Scheme, StatusCode, Trailers,
};
use crate::p3::bindings::http::{handler, types};
use crate::p3::{Fields, WasiHttpImpl, WasiHttpView};

fn get_fields<'a>(
    table: &'a mut ResourceTable,
    fields: &Resource<Fields>,
) -> wasmtime::Result<&'a hyper::HeaderMap> {
    {
        let fields = table
            .get(&fields)
            .context("failed to get fields from table")?;
        if let Fields::Ref { parent, get_fields } = *fields {
            let entry = table
                .get_any_mut(parent)
                .context("failed to get parent fields from table")?;
            return Ok(get_fields(entry));
        }
    }
    match table
        .get_mut(&fields)
        .context("failed to get fields from table")?
    {
        Fields::Owned { fields } => Ok(fields),
        // NB: ideally the `if let` above would go here instead. That makes
        // the borrow-checker unhappy. Unclear why. If you, dear reader, can
        // refactor this to remove the `unreachable!` please do.
        Fields::Ref { .. } => unreachable!(),
    }
}

fn get_fields_mut<'a>(
    table: &'a mut ResourceTable,
    fields: &Resource<Fields>,
) -> wasmtime::Result<&'a mut Fields> {
    table
        .get_mut(fields)
        .context("failed to get fields from table")
}

/// Returns `true` when the header is forbidden according to this [`WasiHttpView`] implementation.
fn is_forbidden_header(view: &mut impl WasiHttpView, name: &hyper::header::HeaderName) -> bool {
    static FORBIDDEN_HEADERS: [hyper::header::HeaderName; 10] = [
        hyper::header::CONNECTION,
        hyper::header::HeaderName::from_static("keep-alive"),
        hyper::header::PROXY_AUTHENTICATE,
        hyper::header::PROXY_AUTHORIZATION,
        hyper::header::HeaderName::from_static("proxy-connection"),
        hyper::header::TE,
        hyper::header::TRANSFER_ENCODING,
        hyper::header::UPGRADE,
        hyper::header::HOST,
        hyper::header::HeaderName::from_static("http2-settings"),
    ];

    FORBIDDEN_HEADERS.contains(name) || view.is_forbidden_header(name)
}

impl<T> types::Host for WasiHttpImpl<T> where T: WasiHttpView {}

impl<T> types::HostFields for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(&mut self) -> wasmtime::Result<Resource<Fields>> {
        self.table()
            .push(Fields::Owned {
                fields: hyper::HeaderMap::new(),
            })
            .context("failed to push fields to table")
    }

    fn from_list(
        &mut self,
        entries: Vec<(FieldName, FieldValue)>,
    ) -> wasmtime::Result<Result<Resource<Fields>, HeaderError>> {
        let mut fields = hyper::HeaderMap::new();

        for (header, value) in entries {
            let header = match hyper::header::HeaderName::from_bytes(header.as_bytes()) {
                Ok(header) => header,
                Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
            };

            if is_forbidden_header(self, &header) {
                return Ok(Err(HeaderError::Forbidden));
            }

            let value = match hyper::header::HeaderValue::from_bytes(&value) {
                Ok(value) => value,
                Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
            };

            fields.append(header, value);
        }
        let fields = self
            .table()
            .push(Fields::Owned { fields })
            .context("failed to push fields to table")?;
        Ok(Ok(fields))
    }

    fn get(
        &mut self,
        fields: Resource<Fields>,
        name: FieldName,
    ) -> wasmtime::Result<Vec<FieldValue>> {
        let fields = get_fields(self.table(), &fields)?;

        let header = match hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(vec![]),
        };

        if !fields.contains_key(&header) {
            return Ok(vec![]);
        }

        let fields = fields
            .get_all(&header)
            .into_iter()
            .map(|val| val.as_bytes().to_owned())
            .collect();
        Ok(fields)
    }

    fn has(&mut self, fields: Resource<Fields>, name: FieldName) -> wasmtime::Result<bool> {
        let fields = get_fields(self.table(), &fields)?;

        match hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => Ok(fields.contains_key(&header)),
            Err(_) => Ok(false),
        }
    }

    fn set(
        &mut self,
        fields: Resource<Fields>,
        name: FieldName,
        value: Vec<FieldValue>,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let header = match hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
        };

        if is_forbidden_header(self, &header) {
            return Ok(Err(HeaderError::Forbidden));
        }

        let mut values = Vec::with_capacity(value.len());
        for value in value {
            match hyper::header::HeaderValue::from_bytes(&value) {
                Ok(value) => values.push(value),
                Err(_) => return Ok(Err(HeaderError::InvalidSyntax)),
            }
        }
        match get_fields_mut(self.table(), &fields)? {
            Fields::Owned { fields } => {
                fields.remove(&header);
                for value in values {
                    fields.append(&header, value);
                }
                Ok(Ok(()))
            }
            Fields::Ref { .. } => Ok(Err(HeaderError::Immutable)),
        }
    }

    fn delete(
        &mut self,
        fields: Resource<Fields>,
        name: FieldName,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let header = match hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(Err(types::HeaderError::InvalidSyntax)),
        };

        if is_forbidden_header(self, &header) {
            return Ok(Err(types::HeaderError::Forbidden));
        }

        match get_fields_mut(self.table(), &fields)? {
            Fields::Owned { fields } => {
                fields.remove(header);
                Ok(Ok(()))
            }
            Fields::Ref { .. } => Ok(Err(HeaderError::Immutable)),
        }
    }

    fn get_and_delete(
        &mut self,
        fields: Resource<Fields>,
        name: FieldName,
    ) -> wasmtime::Result<Result<Vec<FieldValue>, HeaderError>> {
        let Ok(header) = hyper::header::HeaderName::from_bytes(name.as_bytes()) else {
            return Ok(Err(types::HeaderError::InvalidSyntax));
        };

        if is_forbidden_header(self, &header) {
            return Ok(Err(types::HeaderError::Forbidden));
        }

        match get_fields_mut(self.table(), &fields)? {
            Fields::Owned { fields } => {
                let Some(value) = fields.remove(header) else {
                    return Ok(Ok(vec![]));
                };
                todo!("implement");
                Ok(Ok(vec![]))
            }
            Fields::Ref { .. } => Ok(Err(HeaderError::Immutable)),
        }
    }

    fn append(
        &mut self,
        fields: Resource<Fields>,
        name: FieldName,
        value: FieldValue,
    ) -> wasmtime::Result<Result<(), HeaderError>> {
        let header = match hyper::header::HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(Err(types::HeaderError::InvalidSyntax)),
        };

        if is_forbidden_header(self, &header) {
            return Ok(Err(types::HeaderError::Forbidden));
        }

        let value = match hyper::header::HeaderValue::from_bytes(&value) {
            Ok(value) => value,
            Err(_) => return Ok(Err(types::HeaderError::InvalidSyntax)),
        };

        match get_fields_mut(self.table(), &fields)? {
            Fields::Owned { fields } => {
                fields.append(header, value);
                Ok(Ok(()))
            }
            Fields::Ref { .. } => Ok(Err(HeaderError::Immutable)),
        }
    }

    fn entries(
        &mut self,
        fields: Resource<Fields>,
    ) -> wasmtime::Result<Vec<(FieldName, FieldValue)>> {
        todo!()
    }

    fn clone(&mut self, fields: Resource<Fields>) -> wasmtime::Result<Resource<Fields>> {
        todo!()
    }

    fn drop(&mut self, fields: Resource<Fields>) -> wasmtime::Result<()> {
        self.table()
            .delete(fields)
            .context("failed to delete fields from table")?;
        Ok(())
    }
}

impl<T> types::HostBody for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(
        &mut self,
        stream: HostStream<u8>,
        trailers: Option<HostFuture<Resource<Trailers>>>,
    ) -> wasmtime::Result<(Resource<Body>, HostFuture<Result<(), ErrorCode>>)> {
        todo!()
    }

    fn stream(
        &mut self,
        body: Resource<Body>,
    ) -> wasmtime::Result<Result<(HostStream<u8>,), HostFuture<Result<(), ErrorCode>>>> {
        todo!()
    }

    async fn finish<U: 'static>(
        accessor: &mut Accessor<U, Self>,
        this: Resource<Body>,
    ) -> wasmtime::Result<Result<Option<Resource<Trailers>>, ErrorCode>> {
        todo!()
    }

    fn drop(&mut self, body: Resource<Body>) -> wasmtime::Result<()> {
        self.table()
            .delete(body)
            .context("failed to delete body from table")?;
        Ok(())
    }
}

impl<T> types::HostRequest for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(
        &mut self,
        headers: Resource<Headers>,
        body: Option<Resource<Body>>,
        options: Option<Resource<RequestOptions>>,
    ) -> wasmtime::Result<Resource<Request>> {
        todo!()
    }

    fn method(&mut self, req: Resource<Request>) -> wasmtime::Result<Method> {
        todo!()
    }

    fn set_method(
        &mut self,
        req: Resource<Request>,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        todo!()
    }

    fn path_with_query(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<String>> {
        todo!()
    }

    fn set_path_with_query(
        &mut self,
        req: Resource<Request>,
        path_with_query: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        todo!()
    }

    fn scheme(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<Scheme>> {
        todo!()
    }

    fn set_scheme(
        &mut self,
        req: Resource<Request>,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        todo!()
    }

    fn authority(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<String>> {
        todo!()
    }

    fn set_authority(
        &mut self,
        req: Resource<Request>,
        authority: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        todo!()
    }

    fn options(
        &mut self,
        req: Resource<Request>,
    ) -> wasmtime::Result<Option<Resource<RequestOptions>>> {
        todo!()
    }

    fn headers(&mut self, req: Resource<Request>) -> wasmtime::Result<Resource<Headers>> {
        todo!()
    }

    fn body(&mut self, req: Resource<Request>) -> wasmtime::Result<Option<Resource<Body>>> {
        todo!()
    }

    fn into_parts(
        &mut self,
        this: Resource<Request>,
    ) -> wasmtime::Result<(Resource<Headers>, Option<Resource<Body>>)> {
        todo!()
    }

    fn drop(&mut self, req: Resource<Request>) -> wasmtime::Result<()> {
        self.table()
            .delete(req)
            .context("failed to delete request from table")?;
        Ok(())
    }
}

impl<T> types::HostRequestOptions for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(&mut self) -> wasmtime::Result<Resource<RequestOptions>> {
        todo!()
    }

    fn connect_timeout(
        &mut self,
        opts: Resource<RequestOptions>,
    ) -> wasmtime::Result<Option<Duration>> {
        todo!()
    }

    fn set_connect_timeout(
        &mut self,
        opts: Resource<RequestOptions>,
        duration: Option<Duration>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        todo!()
    }

    fn first_byte_timeout(
        &mut self,
        opts: Resource<RequestOptions>,
    ) -> wasmtime::Result<Option<Duration>> {
        todo!()
    }

    fn set_first_byte_timeout(
        &mut self,
        opts: Resource<RequestOptions>,
        duration: Option<Duration>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        todo!()
    }

    fn between_bytes_timeout(
        &mut self,
        opts: Resource<RequestOptions>,
    ) -> wasmtime::Result<Option<Duration>> {
        todo!()
    }

    fn set_between_bytes_timeout(
        &mut self,
        opts: Resource<RequestOptions>,
        duration: Option<Duration>,
    ) -> wasmtime::Result<Result<(), RequestOptionsError>> {
        todo!()
    }

    fn drop(&mut self, opts: Resource<RequestOptions>) -> wasmtime::Result<()> {
        self.table()
            .delete(opts)
            .context("failed to delete request options from table")?;
        Ok(())
    }
}

impl<T> types::HostResponse for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(
        &mut self,
        headers: Resource<Headers>,
        body: Option<Resource<Body>>,
    ) -> wasmtime::Result<Resource<Response>> {
        todo!()
    }

    fn status_code(&mut self, res: Resource<Response>) -> wasmtime::Result<StatusCode> {
        todo!()
    }

    fn set_status_code(
        &mut self,
        res: Resource<Response>,
        status_code: StatusCode,
    ) -> wasmtime::Result<Result<(), ()>> {
        todo!()
    }

    fn headers(&mut self, res: Resource<Response>) -> wasmtime::Result<Resource<Headers>> {
        todo!()
    }

    fn body(&mut self, res: Resource<Response>) -> wasmtime::Result<Option<Resource<Body>>> {
        todo!()
    }

    fn into_parts(
        &mut self,
        this: Resource<Response>,
    ) -> wasmtime::Result<(Resource<Headers>, Option<Resource<Body>>)> {
        todo!()
    }

    fn drop(&mut self, res: Resource<Response>) -> wasmtime::Result<()> {
        self.table()
            .delete(res)
            .context("failed to delete response from table")?;
        Ok(())
    }
}

impl<T> handler::Host for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    async fn handle<U: 'static>(
        store: &mut Accessor<U, Self>,
        request: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<Response>, ErrorCode>> {
        todo!()
    }
}
