use core::ops::{Deref, DerefMut};

use anyhow::Context as _;
use wasmtime::component::Resource;
use wasmtime_wasi::ResourceTable;
use wasmtime_wasi::p3::WithChildren;

use crate::p3::{Request, Response};

mod handler;
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
