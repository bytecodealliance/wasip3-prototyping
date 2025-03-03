use anyhow::Context as _;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use wasmtime::component::{FutureWriter, Promise, Resource};
use wasmtime::AsContextMut;
use wasmtime_wasi::p3::ResourceView;

use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::bindings::Proxy;
use crate::p3::{Request, Response};

impl Proxy {
    /// Call `handle` on [Proxy] getting a [Promise] back.
    async fn handle_promise<T>(
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

    /// Call `handle` on [Proxy].
    pub async fn handle<T>(
        &self,
        mut store: impl AsContextMut<Data = T> + Send + 'static,
        req: impl Into<Request>,
    ) -> wasmtime::Result<
        Result<
            (
                http::Response<BoxBody<Bytes, Option<ErrorCode>>>,
                Option<FutureWriter<Result<(), ErrorCode>>>,
            ),
            ErrorCode,
        >,
    >
    where
        T: ResourceView + Send + 'static,
    {
        let handle = self.handle_promise(&mut store, req).await?;
        match handle.get(&mut store).await? {
            Ok(res) => {
                let res = store
                    .as_context_mut()
                    .data_mut()
                    .table()
                    .delete(res)
                    .context("failed to delete response from table")?;
                res.into_http(store).map(Ok)
            }
            Err(err) => Ok(Err(err)),
        }
    }
}
