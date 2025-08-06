use core::future::Future;

use bytes::Bytes;
use wasmtime::Store;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime_wasi::p3::ResourceView;
use wasmtime_wasi::p3::filesystem::{WasiFilesystemCtx, WasiFilesystemView};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::p3::bindings::http::types::ErrorCode;
use wasmtime_wasi_http::p3::{
    Client, DEFAULT_FORBIDDEN_HEADERS, RequestOptions, WasiHttpCtx, WasiHttpView,
    default_send_request,
};

mod incoming;
mod outgoing;
mod proxy;

struct Ctx<C: Client = TestClient> {
    filesystem: WasiFilesystemCtx,
    table: ResourceTable,
    wasi: WasiCtx,
    http: WasiHttpCtx<C>,
}

impl<C> Default for Ctx<C>
where
    C: Client + Default,
{
    fn default() -> Self {
        Self {
            filesystem: WasiFilesystemCtx::default(),
            table: ResourceTable::default(),
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            http: WasiHttpCtx::default(),
        }
    }
}

impl<C: Client> WasiView for Ctx<C> {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl<C: Client> ResourceView for Ctx<C> {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl<C: Client> WasiFilesystemView for Ctx<C> {
    fn filesystem(&self) -> &WasiFilesystemCtx {
        &self.filesystem
    }
}

impl<C: Client> WasiHttpView for Ctx<C> {
    type Client = C;

    fn http(&self) -> &WasiHttpCtx<C> {
        &self.http
    }

    fn is_forbidden_header(&mut self, name: &http::header::HeaderName) -> bool {
        name.as_str() == "custom-forbidden-header" || DEFAULT_FORBIDDEN_HEADERS.contains(name)
    }
}

#[derive(Clone, Default)]
struct TestClient {
    rejected_authority: Option<String>,
}

impl Client for TestClient {
    type Error = ErrorCode;

    async fn send_request(
        &mut self,
        request: http::Request<
            impl http_body::Body<Data = Bytes, Error = Option<ErrorCode>> + Send + 'static,
        >,
        options: Option<RequestOptions>,
    ) -> wasmtime::Result<
        Result<
            (
                impl Future<
                    Output = Result<
                        http::Response<
                            impl http_body::Body<Data = Bytes, Error = Self::Error> + 'static,
                        >,
                        ErrorCode,
                    >,
                >,
                impl Future<Output = Result<(), Self::Error>> + 'static,
            ),
            ErrorCode,
        >,
    > {
        if let Some(rejected_authority) = &self.rejected_authority {
            let authority = request.uri().authority().map(ToString::to_string).unwrap();
            if &authority == rejected_authority {
                return Ok(Err(ErrorCode::HttpRequestDenied));
            }
        }
        Ok(default_send_request(request, options).await)
    }
}
