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
                "wasi:http/types@0.3.0-draft#[static]body.finish",
                "wasi:http/handler@0.3.0-draft#handle",
            ],
        },
        with: {
            "wasi:http/types/fields": crate::p3::Fields,
        },
    });
}

pub use self::generated::wasi::*;

/// Raw bindings to the `wasi:http/proxy` exports.
pub use self::generated::exports;

/// Bindings to the `wasi:http/proxy` world.
pub use self::generated::{Proxy, ProxyIndices, ProxyPre};
