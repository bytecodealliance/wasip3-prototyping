use core::ops::{Deref, DerefMut};

use anyhow::Context as _;
use wasmtime::component::{Accessor, HostFuture, HostStream, Resource};
use wasmtime_wasi::p3::bindings::clocks::monotonic_clock::Duration;
use wasmtime_wasi::p3::{ResourceView as _, WithChildren};
use wasmtime_wasi::ResourceTable;

use crate::p3::bindings::http::types::{
    ErrorCode, FieldName, FieldValue, HeaderError, Method, RequestOptionsError, Scheme, StatusCode,
    Trailers,
};
use crate::p3::bindings::http::{handler, types};
use crate::p3::{Body, Request, RequestOptions, Response, WasiHttpImpl, WasiHttpView};

fn get_fields<'a>(
    table: &'a ResourceTable,
    fields: &Resource<WithChildren<http::HeaderMap>>,
) -> wasmtime::Result<&'a WithChildren<http::HeaderMap>> {
    table
        .get(&fields)
        .context("failed to get fields from table")
}

fn get_fields_inner<'a>(
    table: &'a ResourceTable,
    fields: &Resource<WithChildren<http::HeaderMap>>,
) -> wasmtime::Result<impl Deref<Target = http::HeaderMap> + use<'a>> {
    let fields = get_fields(table, fields)?;
    fields.get()
}

fn get_fields_mut<'a>(
    table: &'a mut ResourceTable,
    fields: &Resource<WithChildren<http::HeaderMap>>,
) -> wasmtime::Result<&'a mut WithChildren<http::HeaderMap>> {
    table
        .get_mut(&fields)
        .context("failed to get fields from table")
}

fn get_fields_inner_mut<'a>(
    table: &'a mut ResourceTable,
    fields: &Resource<WithChildren<http::HeaderMap>>,
) -> wasmtime::Result<Option<impl DerefMut<Target = http::HeaderMap> + use<'a>>> {
    let fields = get_fields_mut(table, fields)?;
    fields.get_mut()
}

fn push_fields(
    table: &mut ResourceTable,
    fields: WithChildren<http::HeaderMap>,
) -> wasmtime::Result<Resource<WithChildren<http::HeaderMap>>> {
    table.push(fields).context("failed to push fields to table")
}

fn delete_fields(
    table: &mut ResourceTable,
    fields: Resource<WithChildren<http::HeaderMap>>,
) -> wasmtime::Result<WithChildren<http::HeaderMap>> {
    table
        .delete(fields)
        .context("failed to delete fields from table")
}

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

fn get_request<'a>(
    table: &'a ResourceTable,
    req: &Resource<Request>,
) -> wasmtime::Result<&'a Request> {
    table.get(req).context("failed to get request from table")
}

fn get_request_mut<'a>(
    table: &'a mut ResourceTable,
    req: &Resource<Request>,
) -> wasmtime::Result<&'a mut Request> {
    table
        .get_mut(req)
        .context("failed to get request from table")
}

fn push_request(table: &mut ResourceTable, req: Request) -> wasmtime::Result<Resource<Request>> {
    table.push(req).context("failed to push request to table")
}

fn delete_request(table: &mut ResourceTable, req: Resource<Request>) -> wasmtime::Result<Request> {
    table
        .delete(req)
        .context("failed to delete request from table")
}

fn get_response<'a>(
    table: &'a ResourceTable,
    res: &Resource<Response>,
) -> wasmtime::Result<&'a Response> {
    table.get(res).context("failed to get response from table")
}

fn get_response_mut<'a>(
    table: &'a mut ResourceTable,
    res: &Resource<Response>,
) -> wasmtime::Result<&'a mut Response> {
    table
        .get_mut(res)
        .context("failed to get response from table")
}

fn push_response(table: &mut ResourceTable, res: Response) -> wasmtime::Result<Resource<Response>> {
    table.push(res).context("failed to push response to table")
}

fn delete_response(
    table: &mut ResourceTable,
    res: Resource<Response>,
) -> wasmtime::Result<Response> {
    table
        .delete(res)
        .context("failed to delete response from table")
}

fn push_body(table: &mut ResourceTable, body: Body) -> wasmtime::Result<Resource<Body>> {
    table.push(body).context("failed to push body to table")
}

fn delete_body(table: &mut ResourceTable, body: Resource<Body>) -> wasmtime::Result<Body> {
    table
        .delete(body)
        .context("failed to delete body from table")
}

/// Returns `true` when the header is forbidden according to this [`WasiHttpView`] implementation.
fn is_forbidden_header(view: &mut impl WasiHttpView, name: &http::header::HeaderName) -> bool {
    static FORBIDDEN_HEADERS: [http::header::HeaderName; 10] = [
        http::header::CONNECTION,
        http::header::HeaderName::from_static("keep-alive"),
        http::header::PROXY_AUTHENTICATE,
        http::header::PROXY_AUTHORIZATION,
        http::header::HeaderName::from_static("proxy-connection"),
        http::header::TE,
        http::header::TRANSFER_ENCODING,
        http::header::UPGRADE,
        http::header::HOST,
        http::header::HeaderName::from_static("http2-settings"),
    ];

    FORBIDDEN_HEADERS.contains(name) || view.is_forbidden_header(name)
}

impl<T> types::Host for WasiHttpImpl<T> where T: WasiHttpView {}

impl<T> types::HostFields for WasiHttpImpl<T>
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
            Err(_) => return Ok(Err(types::HeaderError::InvalidSyntax)),
        };
        if is_forbidden_header(self, &header) {
            return Ok(Err(types::HeaderError::Forbidden));
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
            return Ok(Err(types::HeaderError::InvalidSyntax));
        };
        if is_forbidden_header(self, &header) {
            return Ok(Err(types::HeaderError::Forbidden));
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
            Err(_) => return Ok(Err(types::HeaderError::InvalidSyntax)),
        };
        if is_forbidden_header(self, &header) {
            return Ok(Err(types::HeaderError::Forbidden));
        }
        let value = match http::header::HeaderValue::from_bytes(&value) {
            Ok(value) => value,
            Err(_) => return Ok(Err(types::HeaderError::InvalidSyntax)),
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

impl<T> types::HostBody for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    #[allow(unused)] // TODO: implement
    async fn new<U>(
        accessor: &mut Accessor<U, Self>,
        stream: HostStream<u8>,
    ) -> wasmtime::Result<(Resource<Body>, HostFuture<Result<(), ErrorCode>>)> {
        anyhow::bail!("TODO")
    }

    #[allow(unused)] // TODO: implement
    async fn new_with_trailers<U>(
        accessor: &mut Accessor<U, Self>,
        stream: HostStream<u8>,
        trailers: HostFuture<Resource<Trailers>>,
    ) -> wasmtime::Result<(Resource<Body>, HostFuture<Result<(), ErrorCode>>)> {
        anyhow::bail!("TODO")
    }

    #[allow(unused)] // TODO: implement
    fn stream(
        &mut self,
        body: Resource<Body>,
    ) -> wasmtime::Result<Result<(HostStream<u8>, HostFuture<Result<(), ErrorCode>>), ()>> {
        anyhow::bail!("TODO")
    }

    #[allow(unused)] // TODO: implement
    async fn finish<U: 'static>(
        accessor: &mut Accessor<U, Self>,
        this: Resource<Body>,
    ) -> wasmtime::Result<HostFuture<Result<Option<Resource<Trailers>>, ErrorCode>>> {
        anyhow::bail!("TODO")
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
        headers: Resource<WithChildren<http::HeaderMap>>,
        body: Option<Resource<Body>>,
        options: Option<Resource<WithChildren<RequestOptions>>>,
    ) -> wasmtime::Result<Resource<Request>> {
        let table = self.table();
        let headers = delete_fields(table, headers)?;
        let headers = headers.unwrap_or_clone()?;
        let body = body.map(|body| delete_body(table, body)).transpose()?;
        let options = options
            .map(|options| {
                let options = delete_request_options(table, options)?;
                options.unwrap_or_clone()
            })
            .transpose()?;
        push_request(table, Request::new(headers, body, options))
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
        push_fields(table, headers.child())
    }

    fn into_parts(
        &mut self,
        req: Resource<Request>,
    ) -> wasmtime::Result<(
        Resource<WithChildren<http::HeaderMap>>,
        Option<Resource<Body>>,
        Option<Resource<WithChildren<RequestOptions>>>,
    )> {
        let table = self.table();
        let Request {
            headers,
            body,
            options,
            ..
        } = delete_request(table, req)?;
        let headers = headers.unwrap_or_clone()?;
        let headers = push_fields(table, WithChildren::new(headers))?;
        let body = body.map(|body| push_body(table, body)).transpose()?;
        let options = options
            .map(|options| {
                let options = options.unwrap_or_clone()?;
                push_request_options(table, WithChildren::new(options))
            })
            .transpose()?;
        Ok((headers, body, options))
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
}

impl<T> types::HostResponse for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    fn new(
        &mut self,
        headers: Resource<WithChildren<http::HeaderMap>>,
        body: Option<Resource<Body>>,
    ) -> wasmtime::Result<Resource<Response>> {
        let table = self.table();
        let headers = delete_fields(table, headers)?;
        let headers = headers.unwrap_or_clone()?;
        let body = body.map(|body| delete_body(table, body)).transpose()?;
        push_response(table, Response::new(headers, body))
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
        push_fields(table, headers.child())
    }

    fn into_parts(
        &mut self,
        res: Resource<Response>,
    ) -> wasmtime::Result<(
        Resource<WithChildren<http::HeaderMap>>,
        Option<Resource<Body>>,
    )> {
        let table = self.table();
        let Response { headers, body, .. } = delete_response(table, res)?;
        let headers = headers.unwrap_or_clone()?;
        let headers = push_fields(table, WithChildren::new(headers))?;
        let body = body.map(|body| push_body(table, body)).transpose()?;
        Ok((headers, body))
    }

    fn drop(&mut self, res: Resource<Response>) -> wasmtime::Result<()> {
        delete_response(self.table(), res)?;
        Ok(())
    }
}

impl<T> handler::Host for WasiHttpImpl<T>
where
    T: WasiHttpView,
{
    #[allow(unused)] // TODO: implement
    async fn handle<U: 'static>(
        store: &mut Accessor<U, Self>,
        request: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<Response>, ErrorCode>> {
        anyhow::bail!("TODO")
    }
}
