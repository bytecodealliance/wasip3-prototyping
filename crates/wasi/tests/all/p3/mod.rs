#![cfg(feature = "p3")]

use std::path::Path;

use anyhow::{Context as _, anyhow};
use test_programs_artifacts::*;
use wasmtime::Store;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi::p3::ResourceView;
use wasmtime_wasi::p3::bindings::Command;
use wasmtime_wasi::p3::cli::{WasiCliCtx, WasiCliView};
use wasmtime_wasi::p3::clocks::{WasiClocksCtx, WasiClocksView};
use wasmtime_wasi::p3::filesystem::{DirPerms, FilePerms, WasiFilesystemCtx, WasiFilesystemView};
use wasmtime_wasi::p3::random::{WasiRandomCtx, WasiRandomView};
use wasmtime_wasi::p3::sockets::{
    AllowedNetworkUses, SocketAddrCheck, WasiSocketsCtx, WasiSocketsView,
};

macro_rules! assert_test_exists {
    ($name:ident) => {
        #[expect(unused_imports, reason = "just here to assert it exists")]
        use self::$name as _;
    };
}

struct Ctx {
    cli: WasiCliCtx,
    clocks: WasiClocksCtx,
    filesystem: WasiFilesystemCtx,
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
            filesystem: WasiFilesystemCtx::default(),
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

impl ResourceView for Ctx {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiCliView for Ctx {
    fn cli(&mut self) -> &WasiCliCtx {
        &self.cli
    }
}

impl WasiClocksView for Ctx {
    fn clocks(&mut self) -> &WasiClocksCtx {
        &self.clocks
    }
}

impl WasiFilesystemView for Ctx {
    fn filesystem(&self) -> &WasiFilesystemCtx {
        &self.filesystem
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
}

async fn run(path: &str) -> anyhow::Result<()> {
    _ = env_logger::try_init();

    let path = Path::new(path);
    let engine = test_programs_artifacts::engine(|config| {
        config.async_support(true);
        config.wasm_component_model_async(true);
    });
    let component = Component::from_file(&engine, path).context("failed to compile component")?;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)
        .context("failed to link `wasi:cli@0.2.x`")?;
    wasmtime_wasi::p3::add_to_linker(&mut linker).context("failed to link `wasi:cli@0.3.x`")?;

    let mut filesystem = WasiFilesystemCtx::default();
    let tempdir = tempfile::Builder::new()
        .prefix(&format!(
            "wasi_components_{}_",
            path.file_stem().unwrap().to_str().unwrap()
        ))
        .tempdir()?;
    println!("preopen: {tempdir:?}");
    filesystem.preopened_dir(tempdir.path(), ".", DirPerms::all(), FilePerms::all())?;
    let mut store = Store::new(
        &engine,
        Ctx {
            filesystem,
            ..Ctx::default()
        },
    );
    let instance = linker.instantiate_async(&mut store, &component).await?;
    let command =
        Command::new(&mut store, &instance).context("failed to instantiate `wasi:cli/command`")?;
    let run = command.wasi_cli_run().call_run(&mut store);
    instance
        .run(&mut store, run)
        .await
        .context("failed to call `wasi:cli/run#run`")?
        .context("guest trapped")?
        .map_err(|()| anyhow!("`wasi:cli/run#run` failed"))
}

foreach_p3!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_clocks_sleep() -> anyhow::Result<()> {
    run(P3_CLOCKS_SLEEP_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_random_imports() -> anyhow::Result<()> {
    run(P3_RANDOM_IMPORTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_ip_name_lookup() -> anyhow::Result<()> {
    run(P3_SOCKETS_IP_NAME_LOOKUP_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_bind() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_BIND_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_connect() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_CONNECT_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_sample_application() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_SAMPLE_APPLICATION_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_sockopts() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_SOCKOPTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_states() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_STATES_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_streams() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_STREAMS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_bind() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_BIND_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_connect() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_CONNECT_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
#[ignore = "https://github.com/bytecodealliance/wasip3-prototyping/issues/44"]
async fn p3_sockets_udp_sample_application() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_SAMPLE_APPLICATION_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_sockopts() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_SOCKOPTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_states() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_STATES_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_filesystem_file_read_write() -> anyhow::Result<()> {
    run(P3_FILESYSTEM_FILE_READ_WRITE_COMPONENT).await
}
