use anyhow::Context as _;
use wasmtime::component::{Promise, Resource};
use wasmtime::AsContextMut;
use wasmtime_wasi::p3::ResourceView;

use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::bindings::Proxy;
use crate::p3::{Request, Response};

impl Proxy {
    /// Call `handle` on [Proxy] getting a [Promise] back.
    pub async fn handle<T>(
        &self,
        mut store: impl AsContextMut<Data = T>,
        req: impl Into<Request>,
    ) -> wasmtime::Result<Promise<Result<Resource<Response>, ErrorCode>>>
    where
        T: ResourceView + Send,
    {
        let mut store = store.as_context_mut();
        let table = store.data_mut().table();
        let req = table
            .push(req.into())
            .context("failed to push request to table")?;
        self.wasi_http_handler().call_handle(&mut store, req).await
    }
}
