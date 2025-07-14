use anyhow::Context as _;
use wasmtime::component::{Accessor, Resource};
use wasmtime_wasi::p3::ResourceView;

use crate::p3::bindings::Proxy;
use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{Request, Response};

impl Proxy {
    /// Call `handle` on [Proxy] getting a [Future] back.
    pub async fn handle<T, R: Into<Request>>(
        &self,
        store: &Accessor<T>,
        req: R,
    ) -> wasmtime::Result<wasmtime::Result<Resource<Response>, ErrorCode>>
    where
        T: ResourceView + Send,
    {
        let req = store.with(|mut store| {
            store
                .data_mut()
                .table()
                .push(req.into())
                .context("failed to push request to table")
        })?;
        self.wasi_http_handler().call_handle(store, req).await
    }
}
