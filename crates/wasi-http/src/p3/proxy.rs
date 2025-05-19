use anyhow::Context as _;
use futures::FutureExt;
use std::future::Future;
use std::pin::Pin;
use wasmtime::AsContextMut;
use wasmtime::component::Resource;
use wasmtime_wasi::p3::ResourceView;

use crate::p3::bindings::Proxy;
use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{Request, Response};

impl Proxy {
    /// Call `handle` on [Proxy] getting a [Future] back.
    pub fn handle<T: 'static, S: AsContextMut<Data = T>, R: Into<Request>>(
        &self,
        mut store: S,
        req: R,
    ) -> wasmtime::Result<
        Pin<
            Box<
                dyn Future<Output = wasmtime::Result<Result<Resource<Response>, ErrorCode>>>
                    + Send
                    + 'static,
            >,
        >,
    >
    where
        T: ResourceView + Send,
    {
        let mut store = store.as_context_mut();
        let table = store.data_mut().table();
        let req = table
            .push(req.into())
            .context("failed to push request to table")?;
        Ok(self
            .wasi_http_handler()
            .call_handle(&mut store, req)
            .boxed())
    }
}
