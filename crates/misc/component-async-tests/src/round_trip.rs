use std::time::Duration;

use wasmtime::component::Accessor;

use super::Ctx;

pub mod bindings {
    wasmtime::component::bindgen!({
        trappable_imports: true,
        path: "wit",
        world: "round-trip",
        concurrent_imports: true,
        concurrent_exports: true,
        async: true,
    });
}

impl bindings::local::local::baz::Host for &mut Ctx {
    async fn foo<T>(_: &mut Accessor<T, Self>, s: String) -> wasmtime::Result<String> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(format!("{s} - entered host - exited host"))
    }
}
