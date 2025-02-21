#![cfg(feature = "p3")]

use anyhow::{anyhow, Context as _};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::Store;
use wasmtime_wasi::p3::bindings::Command;
use wasmtime_wasi::p3::cli::{WasiCliCtx, WasiCliView};
use wasmtime_wasi::p3::clocks::{WasiClocksCtx, WasiClocksView};
use wasmtime_wasi::p3::random::{WasiRandomCtx, WasiRandomView};
use wasmtime_wasi::p3::sockets::{
    AllowedNetworkUses, SocketAddrCheck, WasiSocketsCtx, WasiSocketsView,
};
use wasmtime_wasi::{IoView, WasiCtx, WasiCtxBuilder, WasiView};

macro_rules! assert_test_exists {
    ($name:ident) => {
        #[expect(unused_imports, reason = "just here to assert it exists")]
        use self::$name as _;
    };
}

struct Ctx {
    cli: WasiCliCtx,
    clocks: WasiClocksCtx,
    random: WasiRandomCtx,
    sockets: WasiSocketsCtx,
    table: ResourceTable,
    wasip2: WasiCtx,
}

impl Default for Ctx {
    fn default() -> Self {
        Self {
            cli: WasiCliCtx::default(),
            clocks: WasiClocksCtx::default(),
            sockets: WasiSocketsCtx {
                socket_addr_check: SocketAddrCheck::new(|_, _| Box::pin(async { true })),
                allowed_network_uses: AllowedNetworkUses {
                    ip_name_lookup: true,
                    udp: true,
                    tcp: true,
                },
            },
            random: WasiRandomCtx::default(),
            table: ResourceTable::default(),
            wasip2: WasiCtxBuilder::new().inherit_stdio().build(),
        }
    }
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasip2
    }
}

impl IoView for Ctx {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiCliView for Ctx {
    fn cli(&self) -> &WasiCliCtx {
        &self.cli
    }
}

impl WasiClocksView for Ctx {
    fn clocks(&self) -> &WasiClocksCtx {
        &self.clocks
    }
}

impl WasiRandomView for Ctx {
    fn random(&mut self) -> &mut WasiRandomCtx {
        &mut self.random
    }
}

impl WasiSocketsView for Ctx {
    fn sockets(&self) -> &WasiSocketsCtx {
        &self.sockets
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

fn init_logger() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(pretty_env_logger::init);
}

async fn run(path: &str) -> anyhow::Result<()> {
    init_logger();

    let engine = test_programs_artifacts::engine(|config| {
        config.async_support(true);
        config.wasm_component_model_async(true);
    });
    let component = Component::from_file(&engine, path).context("failed to compile component")?;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker_async(&mut linker).context("failed to link `wasi:cli@0.2.x`")?;
    wasmtime_wasi::p3::add_to_linker(&mut linker).context("failed to link `wasi:cli@0.3.x`")?;

    let mut store = Store::new(&engine, Ctx::default());
    let command = Command::instantiate_async(&mut store, &component, &linker)
        .await
        .context("failed to instantiate `wasi:cli/command`")?;
    let mut promises = wasmtime::component::PromisesUnordered::new();
    let p = command
        .wasi_cli_run()
        .call_run(&mut store)
        .await
        .context("failed to call `wasi:cli/run#run`")?;
    promises.push(p);
    promises
        .next(&mut store)
        .await
        .context("guest trapped")?
        .context("promise missing")?
        .map_err(|()| anyhow!("`wasi:cli/run#run` failed"))
}

mod clocks;
mod random;
mod sockets;
//mod filesystem;
//mod cli;
