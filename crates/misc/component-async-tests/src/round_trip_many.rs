use std::time::Duration;

use anyhow::Result;
use wasmtime::component::Accessor;

use super::Ctx;

pub mod bindings {
    wasmtime::component::bindgen!({
        trappable_imports: true,
        path: "wit",
        world: "round-trip-many",
        concurrent_imports: true,
        concurrent_exports: true,
        async: true,
        additional_derives: [ Eq, PartialEq ],
    });
}

pub mod non_concurrent_export_bindings {
    wasmtime::component::bindgen!({
        trappable_imports: true,
        path: "wit",
        world: "round-trip-many",
        concurrent_imports: true,
        async: true,
        additional_derives: [ Eq, PartialEq ],
    });
}

use bindings::local::local::many::Stuff;

impl bindings::local::local::many::Host for &mut Ctx {
    async fn foo<T>(
        _: &mut Accessor<T, Self>,
        a: String,
        b: u32,
        c: Vec<u8>,
        d: (u64, u64),
        e: Stuff,
        f: Option<Stuff>,
        g: Result<Stuff, ()>,
    ) -> wasmtime::Result<(
        String,
        u32,
        Vec<u8>,
        (u64, u64),
        Stuff,
        Option<Stuff>,
        Result<Stuff, ()>,
    )> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok((
            format!("{a} - entered host - exited host"),
            b,
            c,
            d,
            e,
            f,
            g,
        ))
    }
}
