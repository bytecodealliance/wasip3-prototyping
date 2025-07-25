//! Raw bindings to the `wasi:http` package.

#[expect(missing_docs, reason = "generated code")]
mod generated {
    wasmtime::component::bindgen!({
        path: "src/p3/wit",
        world: "wasi:http/proxy",
        //tracing: true, // TODO: Reenable once fixed
        trappable_imports: true,
        concurrent_exports: true,
        concurrent_imports: true,
        async: {
            only_imports: [
                "wasi:http/handler@0.3.0-draft#[async]handle",
                "wasi:http/types@0.3.0-draft#[method]request.body",
                "wasi:http/types@0.3.0-draft#[method]response.body",
                "wasi:http/types@0.3.0-draft#[static]request.new",
                "wasi:http/types@0.3.0-draft#[drop]request",
                "wasi:http/types@0.3.0-draft#[static]response.new",
                "wasi:http/types@0.3.0-draft#[drop]response",
            ],
        },
        with: {
            "wasi:http/types/fields": with::Fields,
            "wasi:http/types/request": crate::p3::Request,
            "wasi:http/types/request-options": with::RequestOptions,
            "wasi:http/types/response": crate::p3::Response,
        },
    });

    mod with {
        /// The concrete type behind a `wasi:http/types/fields` resource.
        pub type Fields = wasmtime_wasi::p3::WithChildren<http::HeaderMap>;

        /// The concrete type behind a `wasi:http/types/request-options` resource.
        pub type RequestOptions = wasmtime_wasi::p3::WithChildren<crate::p3::RequestOptions>;
    }
}

pub use self::generated::wasi::*;

/// Raw bindings to the `wasi:http/proxy` exports.
pub use self::generated::exports;

/// Bindings to the `wasi:http/proxy` world.
pub use self::generated::{Proxy, ProxyIndices, ProxyPre};
