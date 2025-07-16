use core::future::Future;

use bytes::Bytes;
use wasmtime::Store;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime_wasi::clocks::{WasiClocksCtx, WasiClocksView};
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi::p3::ResourceView;
use wasmtime_wasi::p3::cli::{WasiCliCtx, WasiCliView};
use wasmtime_wasi::p3::filesystem::{WasiFilesystemCtx, WasiFilesystemView};
use wasmtime_wasi::p3::sockets::{WasiSocketsCtx, WasiSocketsView};
use wasmtime_wasi::random::{WasiRandomCtx, WasiRandomView};
use wasmtime_wasi_http::p3::bindings::http::types::ErrorCode;
use wasmtime_wasi_http::p3::{
    Client, DEFAULT_FORBIDDEN_HEADERS, RequestOptions, WasiHttpCtx, WasiHttpView,
    default_send_request,
};

mod incoming;
mod outgoing;
mod proxy;

struct Ctx<C: Client = TestClient> {
    cli: WasiCliCtx,
    filesystem: WasiFilesystemCtx,
    sockets: WasiSocketsCtx,
    table: ResourceTable,
    wasip2: WasiCtx,
    wasip3: wasmtime_wasi::p3::WasiCtx,
    http: WasiHttpCtx<C>,
}

impl<C> Default for Ctx<C>
where
    C: Client + Default,
{
    fn default() -> Self {
        Self {
            cli: WasiCliCtx::default(),
            filesystem: WasiFilesystemCtx::default(),
            sockets: WasiSocketsCtx::default(),
            table: ResourceTable::default(),
            wasip2: WasiCtxBuilder::new().inherit_stdio().build(),
            wasip3: wasmtime_wasi::p3::WasiCtxBuilder::new()
                .inherit_stdio()
                .build(),
            http: WasiHttpCtx::default(),
        }
    }
}

impl<C: Client> WasiView for Ctx<C> {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasip2
    }
}

impl<C: Client> IoView for Ctx<C> {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl<C: Client> wasmtime_wasi::p3::WasiView for Ctx<C> {
    fn ctx(&mut self) -> &mut wasmtime_wasi::p3::WasiCtx {
        &mut self.wasip3
    }
}

impl<C: Client> ResourceView for Ctx<C> {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl<C: Client> WasiCliView for Ctx<C> {
    fn cli(&mut self) -> &WasiCliCtx {
        &self.cli
    }
}

impl<C: Client> WasiClocksView for Ctx<C> {
    fn clocks(&mut self) -> &WasiClocksCtx {
        &self.wasip3.clocks
    }
}

impl<C: Client> WasiFilesystemView for Ctx<C> {
    fn filesystem(&self) -> &WasiFilesystemCtx {
        &self.filesystem
    }
}

impl<C: Client> WasiRandomView for Ctx<C> {
    fn random(&mut self) -> &mut WasiRandomCtx {
        &mut self.wasip3.random
    }
}

impl<C: Client> WasiSocketsView for Ctx<C> {
    fn sockets(&self) -> &WasiSocketsCtx {
        &self.sockets
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
