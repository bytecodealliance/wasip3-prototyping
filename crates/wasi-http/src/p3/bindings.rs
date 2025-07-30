//! Raw bindings to the `wasi:http` package.

#[expect(missing_docs, reason = "generated code")]
mod generated {
    wasmtime::component::bindgen!({
        path: "src/p3/wit",
        world: "wasi:http/proxy",
        imports: {
            "wasi:http/handler/[async]handle": async | store | trappable | tracing,
            "wasi:http/types/[method]request.body": async | store | trappable | tracing,
            "wasi:http/types/[method]response.body": async | store | trappable | tracing,
            "wasi:http/types/[static]request.new": async | store | trappable | tracing,
            "wasi:http/types/[drop]request": async | store | trappable | tracing,
            "wasi:http/types/[static]response.new": async | store | trappable | tracing,
            "wasi:http/types/[drop]response": async | store | trappable | tracing,
            default: trappable | tracing,
        },
        exports: { default: async | store },
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
