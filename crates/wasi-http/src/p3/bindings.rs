//! Raw bindings to the `wasi:http` package.

#[allow(missing_docs)]
mod generated {
    wasmtime::component::bindgen!({
        path: "src/p3/wit",
        world: "wasi:http/proxy",
        //tracing: true, // TODO: Reenable once fixed
        trappable_imports: true,
        concurrent_imports: true,
        async: {
            only_imports: [
                "wasi:http/types@0.3.0-draft#[static]body.new",
                "wasi:http/types@0.3.0-draft#[static]body.new-with-trailers",
                "wasi:http/types@0.3.0-draft#[static]body.finish",
                "wasi:http/handler@0.3.0-draft#handle",
            ],
        },
        with: {
            "wasi:http/types/body": crate::p3::Body,
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
