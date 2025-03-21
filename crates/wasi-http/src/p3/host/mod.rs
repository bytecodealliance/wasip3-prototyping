use core::ops::{Deref, DerefMut};

use anyhow::Context as _;
use wasmtime::component::Resource;
use wasmtime_wasi::p3::WithChildren;
use wasmtime_wasi::ResourceTable;

use crate::p3::{Request, RequestOptions, Response, WasiHttpView};

mod handle;
mod types;

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

fn push_fields_child<T: 'static>(
    table: &mut ResourceTable,
    fields: WithChildren<http::HeaderMap>,
    parent: &Resource<T>,
) -> wasmtime::Result<Resource<WithChildren<http::HeaderMap>>> {
    table
        .push_child(fields, parent)
        .context("failed to push fields to table")
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
