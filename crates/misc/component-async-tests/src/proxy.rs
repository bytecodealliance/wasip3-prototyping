use anyhow::{anyhow, Result};
use wasi_http_draft::wasi::http::types::{ErrorCode, Request, Response};
use wasi_http_draft::{WasiHttp, WasiHttpView, WasiHttpViewConcurrent};
use wasmtime::component::{Accessor, Resource, ResourceTable};

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "wasi:http/proxy",
        concurrent_imports: true,
        concurrent_exports: true,
        async: {
            only_imports: [
                "wasi:http/types@0.3.0-draft#[static]body.finish",
                "wasi:http/handler@0.3.0-draft#handle",
            ]
        },
        with: {
            "wasi:http/types": wasi_http_draft::wasi::http::types,
        }
    });
}

impl WasiHttpView for super::Ctx {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiHttpViewConcurrent for super::Ctx {
    type View<'a> = &'a mut Self;

    async fn send_request<T>(
        _accessor: &mut Accessor<T, WasiHttp<Self>>,
        _request: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<Response>, ErrorCode>> {
        Err(anyhow!("no outbound request handler available"))
    }
}
