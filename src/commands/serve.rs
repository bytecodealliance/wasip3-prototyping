use crate::common::{Profile, RunCommon, RunTarget};
use anyhow::{anyhow, bail, Context as _, Result};
use clap::Parser;
use http_body_util::BodyExt as _;
use std::{net::SocketAddr, sync::Mutex};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::io::{stderr, stdin, stdout};
use wasmtime::component::Linker;
use wasmtime::{Engine, Store, StoreLimits};
use wasmtime_wasi::{IoView, StreamError, StreamResult, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi_http::bindings::http::types::Scheme;
use wasmtime_wasi_http::bindings::ProxyPre;
use wasmtime_wasi_http::io::TokioIo;
use wasmtime_wasi_http::{
    body::HyperOutgoingBody, WasiHttpCtx, WasiHttpView, DEFAULT_OUTGOING_BODY_BUFFER_CHUNKS,
    DEFAULT_OUTGOING_BODY_CHUNK_SIZE,
};

#[cfg(feature = "wasi-config")]
use wasmtime_wasi_config::{WasiConfig, WasiConfigVariables};
#[cfg(feature = "wasi-keyvalue")]
use wasmtime_wasi_keyvalue::{WasiKeyValue, WasiKeyValueCtx, WasiKeyValueCtxBuilder};
#[cfg(feature = "wasi-nn")]
use wasmtime_wasi_nn::wit::WasiNnCtx;

struct Host {
    table: wasmtime::component::ResourceTable,
    ctx: WasiCtx,
    http: WasiHttpCtx,
    http_outgoing_body_buffer_chunks: Option<usize>,
    http_outgoing_body_chunk_size: Option<usize>,

    p3_cli: Option<Arc<Mutex<wasmtime_wasi::p3::cli::WasiCliCtx>>>,
    p3_clocks: Option<Arc<Mutex<wasmtime_wasi::p3::clocks::WasiClocksCtx>>>,
    p3_filesystem: Option<wasmtime_wasi::p3::filesystem::WasiFilesystemCtx>,
    p3_random: Option<Arc<Mutex<wasmtime_wasi::p3::random::WasiRandomCtx>>>,
    p3_sockets: Option<wasmtime_wasi::p3::sockets::WasiSocketsCtx>,
    p3_http: wasmtime_wasi_http::p3::WasiHttpCtx,

    limits: StoreLimits,

    #[cfg(feature = "wasi-nn")]
    nn: Option<WasiNnCtx>,

    #[cfg(feature = "wasi-config")]
    wasi_config: Option<WasiConfigVariables>,

    #[cfg(feature = "wasi-keyvalue")]
    wasi_keyvalue: Option<WasiKeyValueCtx>,
}

impl IoView for Host {
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
}
impl WasiView for Host {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

impl WasiHttpView for Host {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }

    fn outgoing_body_buffer_chunks(&mut self) -> usize {
        self.http_outgoing_body_buffer_chunks
            .unwrap_or_else(|| DEFAULT_OUTGOING_BODY_BUFFER_CHUNKS)
    }

    fn outgoing_body_chunk_size(&mut self) -> usize {
        self.http_outgoing_body_chunk_size
            .unwrap_or_else(|| DEFAULT_OUTGOING_BODY_CHUNK_SIZE)
    }
}

impl wasmtime_wasi::p3::ResourceView for Host {
    fn table(&mut self) -> &mut wasmtime::component::ResourceTable {
        &mut self.table
    }
}
impl wasmtime_wasi::p3::cli::WasiCliView for Host {
    fn cli(&mut self) -> &wasmtime_wasi::p3::cli::WasiCliCtx {
        let cli = self
            .p3_cli
            .as_mut()
            .and_then(Arc::get_mut)
            .expect("`wasi:cli@0.3` not configured");
        cli.get_mut().unwrap()
    }
}

impl wasmtime_wasi::p3::clocks::WasiClocksView for Host {
    fn clocks(&mut self) -> &wasmtime_wasi::p3::clocks::WasiClocksCtx {
        let clocks = self
            .p3_clocks
            .as_mut()
            .and_then(Arc::get_mut)
            .expect("`wasi:clocks@0.3` not configured");
        clocks.get_mut().unwrap()
    }
}

impl wasmtime_wasi::p3::filesystem::WasiFilesystemView for Host {
    fn filesystem(&self) -> &wasmtime_wasi::p3::filesystem::WasiFilesystemCtx {
        self.p3_filesystem
            .as_ref()
            .expect("`wasi:filesystem@0.3` not configured")
    }
}

impl wasmtime_wasi::p3::random::WasiRandomView for Host {
    fn random(&mut self) -> &mut wasmtime_wasi::p3::random::WasiRandomCtx {
        let random = self
            .p3_random
            .as_mut()
            .and_then(Arc::get_mut)
            .expect("`wasi:random@0.3` not configured");
        random.get_mut().unwrap()
    }
}

impl wasmtime_wasi::p3::sockets::WasiSocketsView for Host {
    fn sockets(&self) -> &wasmtime_wasi::p3::sockets::WasiSocketsCtx {
        self.p3_sockets
            .as_ref()
            .expect("`wasi:sockets@0.3` not configured")
    }
}

impl wasmtime_wasi_http::p3::WasiHttpView for Host {
    type Client = wasmtime_wasi_http::p3::DefaultClient;

    fn http(&self) -> &wasmtime_wasi_http::p3::WasiHttpCtx<Self::Client> {
        &self.p3_http
    }
}

const DEFAULT_ADDR: std::net::SocketAddr = std::net::SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
    8080,
);

/// Runs a WebAssembly module
#[derive(Parser)]
pub struct ServeCommand {
    #[command(flatten)]
    run: RunCommon,

    /// Socket address for the web server to bind to.
    #[arg(long = "addr", value_name = "SOCKADDR", default_value_t = DEFAULT_ADDR)]
    addr: SocketAddr,

    /// Disable log prefixes of wasi-http handlers.
    /// if unspecified, logs will be prefixed with 'stdout|stderr [{req_id}] :: '
    #[arg(long = "no-logging-prefix")]
    no_logging_prefix: bool,

    /// The WebAssembly component to run.
    #[arg(value_name = "WASM", required = true)]
    component: PathBuf,
}

impl ServeCommand {
    /// Start a server to run the given wasi-http proxy component
    pub fn execute(mut self) -> Result<()> {
        self.run.common.init_logging()?;

        // We force cli errors before starting to listen for connections so then
        // we don't accidentally delay them to the first request.
        if let Some(Profile::Guest { .. }) = &self.run.profile {
            bail!("Cannot use the guest profiler with components");
        }

        if self.run.common.wasi.nn == Some(true) {
            #[cfg(not(feature = "wasi-nn"))]
            {
                bail!("Cannot enable wasi-nn when the binary is not compiled with this feature.");
            }
        }

        if self.run.common.wasi.threads == Some(true) {
            bail!("wasi-threads does not support components yet")
        }

        // The serve command requires both wasi-http and the component model, so
        // we enable those by default here.
        if self.run.common.wasi.http.replace(true) == Some(false) {
            bail!("wasi-http is required for the serve command, and must not be disabled");
        }
        if self.run.common.wasm.component_model.replace(true) == Some(false) {
            bail!("components are required for the serve command, and must not be disabled");
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_time()
            .enable_io()
            .build()?;

        runtime.block_on(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    Ok::<_, anyhow::Error>(())
                }

                res = self.serve() => {
                    res
                }
            }
        })?;

        Ok(())
    }

    fn set_p3_ctx(&self, store: &mut Store<Host>) -> Result<()> {
        store.data_mut().p3_clocks = Some(Arc::default());
        store.data_mut().p3_random = Some(Arc::default());

        let mut environment = Vec::default();
        if self.run.common.wasi.inherit_env == Some(true) {
            for (k, v) in std::env::vars() {
                environment.push((k, v));
            }
        }
        for (key, value) in self.run.vars.iter() {
            let value = match value {
                Some(value) => value.clone(),
                None => match std::env::var_os(key) {
                    Some(val) => val
                        .into_string()
                        .map_err(|_| anyhow!("environment variable `{key}` not valid utf-8"))?,
                    None => {
                        // leave the env var un-set in the guest
                        continue;
                    }
                },
            };
            environment.push((key.clone(), value));
        }
        store.data_mut().p3_cli = Some(Arc::new(Mutex::new(wasmtime_wasi::p3::cli::WasiCliCtx {
            environment,
            arguments: vec![],
            initial_cwd: None,
            stdin: Box::new(stdin()),
            stdout: Box::new(stdout()),
            stderr: Box::new(stderr()),
        })));

        let mut p3_filesystem = wasmtime_wasi::p3::filesystem::WasiFilesystemCtx::default();
        p3_filesystem.allow_blocking_current_thread = self.run.common.wasm.timeout.is_none();
        for (host, guest) in self.run.dirs.iter() {
            p3_filesystem.preopened_dir(
                host,
                guest,
                wasmtime_wasi::p3::filesystem::DirPerms::all(),
                wasmtime_wasi::p3::filesystem::FilePerms::all(),
            )?;
        }
        store.data_mut().p3_filesystem = Some(p3_filesystem);

        if self.run.common.wasi.listenfd == Some(true) {
            bail!("components do not support --listenfd");
        }
        for _ in self.run.compute_preopen_sockets()? {
            bail!("components do not support --tcplisten");
        }

        let mut p3_sockets = wasmtime_wasi::p3::sockets::WasiSocketsCtx::default();
        if self.run.common.wasi.inherit_network == Some(true) {
            p3_sockets.socket_addr_check =
                wasmtime_wasi::p3::sockets::SocketAddrCheck::new(|_, _| Box::pin(async { true }))
        }
        if let Some(enable) = self.run.common.wasi.allow_ip_name_lookup {
            p3_sockets.allowed_network_uses.ip_name_lookup = enable;
        }
        if let Some(enable) = self.run.common.wasi.tcp {
            p3_sockets.allowed_network_uses.tcp = enable;
        }
        if let Some(enable) = self.run.common.wasi.udp {
            p3_sockets.allowed_network_uses.udp = enable;
        }
        store.data_mut().p3_sockets = Some(p3_sockets);

        Ok(())
    }

    fn new_store(&self, engine: &Engine, req_id: u64) -> Result<Store<Host>> {
        let mut builder = WasiCtxBuilder::new();
        self.run.configure_wasip2(&mut builder)?;

        builder.env("REQUEST_ID", req_id.to_string());

        let stdout_prefix: String;
        let stderr_prefix: String;
        if self.no_logging_prefix {
            stdout_prefix = "".to_string();
            stderr_prefix = "".to_string();
        } else {
            stdout_prefix = format!("stdout [{req_id}] :: ");
            stderr_prefix = format!("stderr [{req_id}] :: ");
        }
        builder.stdout(LogStream::new(stdout_prefix, Output::Stdout));
        builder.stderr(LogStream::new(stderr_prefix, Output::Stderr));

        let mut host = Host {
            table: wasmtime::component::ResourceTable::new(),
            ctx: builder.build(),
            http: WasiHttpCtx::new(),
            http_outgoing_body_buffer_chunks: self.run.common.wasi.http_outgoing_body_buffer_chunks,
            http_outgoing_body_chunk_size: self.run.common.wasi.http_outgoing_body_chunk_size,

            limits: StoreLimits::default(),

            #[cfg(feature = "wasi-nn")]
            nn: None,
            #[cfg(feature = "wasi-config")]
            wasi_config: None,
            #[cfg(feature = "wasi-keyvalue")]
            wasi_keyvalue: None,

            p3_cli: None,
            p3_clocks: None,
            p3_filesystem: None,
            p3_random: None,
            p3_sockets: None,
            p3_http: wasmtime_wasi_http::p3::WasiHttpCtx::default(),
        };

        if self.run.common.wasi.nn == Some(true) {
            #[cfg(feature = "wasi-nn")]
            {
                let graphs = self
                    .run
                    .common
                    .wasi
                    .nn_graph
                    .iter()
                    .map(|g| (g.format.clone(), g.dir.clone()))
                    .collect::<Vec<_>>();
                let (backends, registry) = wasmtime_wasi_nn::preload(&graphs)?;
                host.nn.replace(WasiNnCtx::new(backends, registry));
            }
        }

        if self.run.common.wasi.config == Some(true) {
            #[cfg(feature = "wasi-config")]
            {
                let vars = WasiConfigVariables::from_iter(
                    self.run
                        .common
                        .wasi
                        .config_var
                        .iter()
                        .map(|v| (v.key.clone(), v.value.clone())),
                );
                host.wasi_config.replace(vars);
            }
        }

        if self.run.common.wasi.keyvalue == Some(true) {
            #[cfg(feature = "wasi-keyvalue")]
            {
                let ctx = WasiKeyValueCtxBuilder::new()
                    .in_memory_data(
                        self.run
                            .common
                            .wasi
                            .keyvalue_in_memory_data
                            .iter()
                            .map(|v| (v.key.clone(), v.value.clone())),
                    )
                    .build();
                host.wasi_keyvalue.replace(ctx);
            }
        }

        let mut store = Store::new(engine, host);
        self.set_p3_ctx(&mut store)?;

        if self.run.common.wasm.timeout.is_some() {
            store.set_epoch_deadline(u64::from(EPOCH_PRECISION) + 1);
        }

        store.data_mut().limits = self.run.store_limits();
        store.limiter(|t| &mut t.limits);

        // If fuel has been configured, we want to add the configured
        // fuel amount to this store.
        if let Some(fuel) = self.run.common.wasm.fuel {
            store.set_fuel(fuel)?;
        }

        Ok(store)
    }

    fn add_to_linker(&self, linker: &mut Linker<Host>) -> Result<()> {
        let mut cli = self.run.common.wasi.cli;

        // Accept -Scommon as a deprecated alias for -Scli.
        if let Some(common) = self.run.common.wasi.common {
            if cli.is_some() {
                bail!(
                    "The -Scommon option should not be use with -Scli as it is a deprecated alias"
                );
            } else {
                // In the future, we may add a warning here to tell users to use
                // `-S cli` instead of `-S common`.
                cli = Some(common);
            }
        }

        // Repurpose the `-Scli` flag of `wasmtime run` for `wasmtime serve`
        // to serve as a signal to enable all WASI interfaces instead of just
        // those in the `proxy` world. If `-Scli` is present then add all
        // `command` APIs and then additionally add in the required HTTP APIs.
        //
        // If `-Scli` isn't passed then use the `add_to_linker_async`
        // bindings which adds just those interfaces that the proxy interface
        // uses.
        if cli == Some(true) {
            let link_options = self.run.compute_wasi_features();
            wasmtime_wasi::add_to_linker_with_options_async(linker, &link_options)?;
            wasmtime_wasi_http::add_only_http_to_linker_async(linker)?;
        } else {
            wasmtime_wasi_http::add_to_linker_async(linker)?;
        }

        if self.run.common.wasi.nn == Some(true) {
            #[cfg(not(feature = "wasi-nn"))]
            {
                bail!("support for wasi-nn was disabled at compile time");
            }
            #[cfg(feature = "wasi-nn")]
            {
                wasmtime_wasi_nn::wit::add_to_linker(linker, |h: &mut Host| {
                    let ctx = h.nn.as_mut().unwrap();
                    wasmtime_wasi_nn::wit::WasiNnView::new(&mut h.table, ctx)
                })?;
            }
        }

        if self.run.common.wasi.config == Some(true) {
            #[cfg(not(feature = "wasi-config"))]
            {
                bail!("support for wasi-config was disabled at compile time");
            }
            #[cfg(feature = "wasi-config")]
            {
                wasmtime_wasi_config::add_to_linker(linker, |h| {
                    WasiConfig::from(h.wasi_config.as_ref().unwrap())
                })?;
            }
        }

        if self.run.common.wasi.keyvalue == Some(true) {
            #[cfg(not(feature = "wasi-keyvalue"))]
            {
                bail!("support for wasi-keyvalue was disabled at compile time");
            }
            #[cfg(feature = "wasi-keyvalue")]
            {
                wasmtime_wasi_keyvalue::add_to_linker(linker, |h: &mut Host| {
                    WasiKeyValue::new(h.wasi_keyvalue.as_ref().unwrap(), &mut h.table)
                })?;
            }
        }

        if self.run.common.wasi.threads == Some(true) {
            bail!("support for wasi-threads is not available with components");
        }

        if self.run.common.wasi.http == Some(false) {
            bail!("support for wasi-http must be enabled for `serve` subcommand");
        }

        Ok(())
    }

    async fn serve(mut self) -> Result<()> {
        use hyper::server::conn::http1;

        let mut config = self
            .run
            .common
            .config(use_pooling_allocator_by_default().unwrap_or(None))?;
        config.wasm_component_model(true);
        config.async_support(true);

        if self.run.common.wasm.timeout.is_some() {
            config.epoch_interruption(true);
        }

        match self.run.profile {
            Some(Profile::Native(s)) => {
                config.profiler(s);
            }

            // We bail early in `execute` if the guest profiler is configured.
            Some(Profile::Guest { .. }) => unreachable!(),

            None => {}
        }

        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);

        self.add_to_linker(&mut linker)?;

        let component = match self.run.load_module(&engine, &self.component)? {
            RunTarget::Core(_) => bail!("The serve command currently requires a component"),
            RunTarget::Component(c) => c,
        };
        let instance = linker.instantiate_pre(&component)?;

        let socket = match &self.addr {
            SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
            SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
        };
        // Conditionally enable `SO_REUSEADDR` depending on the current
        // platform. On Unix we want this to be able to rebind an address in
        // the `TIME_WAIT` state which can happen then a server is killed with
        // active TCP connections and then restarted. On Windows though if
        // `SO_REUSEADDR` is specified then it enables multiple applications to
        // bind the port at the same time which is not something we want. Hence
        // this is conditionally set based on the platform (and deviates from
        // Tokio's default from always-on).
        socket.set_reuseaddr(!cfg!(windows))?;
        socket.bind(self.addr)?;
        let listener = socket.listen(100)?;

        eprintln!("Serving HTTP on http://{}/", listener.local_addr()?);

        let _epoch_thread = if let Some(timeout) = self.run.common.wasm.timeout {
            Some(EpochThread::spawn(
                timeout / EPOCH_PRECISION,
                engine.clone(),
            ))
        } else {
            None
        };

        log::info!("Listening on {}", self.addr);

        if let Ok(instance) = wasmtime_wasi_http::p3::bindings::ProxyPre::new(instance.clone()) {
            let next_id = AtomicU64::default();
            loop {
                let (stream, _) = listener.accept().await?;
                tokio::task::spawn(async {
                    if let Err(e) = http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(
                            TokioIo::new(stream),
                            hyper::service::service_fn(move |req| {
let instance = instance.clone();
                                let engine = engine.clone();
async move {
                                let req_id = next_id.fetch_add(1, Ordering::Relaxed);
                                let mut store = self.new_store(&engine, req_id).context("failed")?;
                                let proxy = instance.instantiate_async(&mut store).await?;
                                let res = proxy
                                    .handle(
                                        store,
                                        req.map(|body: hyper::body::Incoming| {
                                            body.map_err(|err| {
                                                eprintln!("TODO: convert error {err:?}");
                                                wasmtime_wasi_http::p3::bindings::http::types::ErrorCode::InternalError(None)
                                            })
                                        }),
                                    )
                                    .await.context("failed")?.context("failed")?;
                                anyhow::Ok(res.map(|body| body.map_err(|err| err.unwrap())))
                            }
                            }),
                        )
                        .await
                    {
                        eprintln!("error: {e:?}");
                    }
                });
            }
        }

        let instance = wasmtime_wasi_http::bindings::ProxyPre::new(instance)?;

        let handler = ProxyHandler::new(self, engine, instance);

        loop {
            let (stream, _) = listener.accept().await?;
            let stream = TokioIo::new(stream);
            let h = handler.clone();
            tokio::task::spawn(async {
                if let Err(e) = http1::Builder::new()
                    .keep_alive(true)
                    .serve_connection(
                        stream,
                        hyper::service::service_fn(move |req| handle_request(h.clone(), req)),
                    )
                    .await
                {
                    eprintln!("error: {e:?}");
                }
            });
        }
    }
}

/// This is the number of epochs that we will observe before expiring a request handler. As
/// instances may be started at any point within an epoch, and epochs are counted globally per
/// engine, we expire after `EPOCH_PRECISION + 1` epochs have been observed. This gives a maximum
/// overshoot of `timeout / EPOCH_PRECISION`, which is more desirable than expiring early.
const EPOCH_PRECISION: u32 = 10;

struct EpochThread {
    shutdown: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl EpochThread {
    fn spawn(timeout: std::time::Duration, engine: Engine) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = {
            let shutdown = Arc::clone(&shutdown);
            let handle = std::thread::spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    std::thread::sleep(timeout);
                    engine.increment_epoch();
                }
            });
            Some(handle)
        };

        EpochThread { shutdown, handle }
    }
}

impl Drop for EpochThread {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.shutdown.store(true, Ordering::Relaxed);
            handle.join().unwrap();
        }
    }
}

struct ProxyHandlerInner {
    cmd: ServeCommand,
    engine: Engine,
    instance_pre: ProxyPre<Host>,
    next_id: AtomicU64,
}

impl ProxyHandlerInner {
    fn next_req_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Clone)]
struct ProxyHandler(Arc<ProxyHandlerInner>);

impl ProxyHandler {
    fn new(cmd: ServeCommand, engine: Engine, instance_pre: ProxyPre<Host>) -> Self {
        Self(Arc::new(ProxyHandlerInner {
            cmd,
            engine,
            instance_pre,
            next_id: AtomicU64::from(0),
        }))
    }
}

type Request = hyper::Request<hyper::body::Incoming>;

async fn handle_request(
    ProxyHandler(inner): ProxyHandler,
    req: Request,
) -> Result<hyper::Response<HyperOutgoingBody>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();

    let req_id = inner.next_req_id();

    log::info!(
        "Request {req_id} handling {} to {}",
        req.method(),
        req.uri()
    );

    let mut store = inner.cmd.new_store(&inner.engine, req_id)?;

    let req = store.data_mut().new_incoming_request(Scheme::Http, req)?;
    let out = store.data_mut().new_response_outparam(sender)?;
    let proxy = inner.instance_pre.instantiate_async(&mut store).await?;

    let task = tokio::task::spawn(async move {
        if let Err(e) = proxy
            .wasi_http_incoming_handler()
            .call_handle(store, req, out)
            .await
        {
            log::error!("[{req_id}] :: {:?}", e);
            return Err(e);
        }

        Ok(())
    });

    match receiver.await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            // An error in the receiver (`RecvError`) only indicates that the
            // task exited before a response was sent (i.e., the sender was
            // dropped); it does not describe the underlying cause of failure.
            // Instead we retrieve and propagate the error from inside the task
            // which should more clearly tell the user what went wrong. Note
            // that we assume the task has already exited at this point so the
            // `await` should resolve immediately.
            let e = match task.await {
                Ok(r) => r.expect_err("if the receiver has an error, the task must have failed"),
                Err(e) => e.into(),
            };
            return Err(e.context("guest never invoked `response-outparam::set` method"));
        }
    }
}

#[derive(Clone)]
enum Output {
    Stdout,
    Stderr,
}

impl Output {
    fn write_all(&self, buf: &[u8]) -> anyhow::Result<()> {
        use std::io::Write;

        match self {
            Output::Stdout => std::io::stdout().write_all(buf),
            Output::Stderr => std::io::stderr().write_all(buf),
        }
        .map_err(|e| anyhow!(e))
    }
}

#[derive(Clone)]
struct LogStream {
    prefix: String,
    output: Output,
    needs_prefix_on_next_write: bool,
}

impl LogStream {
    fn new(prefix: String, output: Output) -> LogStream {
        LogStream {
            prefix,
            output,
            needs_prefix_on_next_write: true,
        }
    }
}

impl wasmtime_wasi::StdoutStream for LogStream {
    fn stream(&self) -> Box<dyn wasmtime_wasi::OutputStream> {
        Box::new(self.clone())
    }

    fn isatty(&self) -> bool {
        use std::io::IsTerminal;

        match &self.output {
            Output::Stdout => std::io::stdout().is_terminal(),
            Output::Stderr => std::io::stderr().is_terminal(),
        }
    }
}

impl wasmtime_wasi::OutputStream for LogStream {
    fn write(&mut self, bytes: bytes::Bytes) -> StreamResult<()> {
        let mut bytes = &bytes[..];

        while !bytes.is_empty() {
            if self.needs_prefix_on_next_write {
                self.output
                    .write_all(self.prefix.as_bytes())
                    .map_err(StreamError::LastOperationFailed)?;
                self.needs_prefix_on_next_write = false;
            }
            match bytes.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    let (a, b) = bytes.split_at(i + 1);
                    bytes = b;
                    self.output
                        .write_all(a)
                        .map_err(StreamError::LastOperationFailed)?;
                    self.needs_prefix_on_next_write = true;
                }
                None => {
                    self.output
                        .write_all(bytes)
                        .map_err(StreamError::LastOperationFailed)?;
                    break;
                }
            }
        }

        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(1024 * 1024)
    }
}

#[async_trait::async_trait]
impl wasmtime_wasi::Pollable for LogStream {
    async fn ready(&mut self) {}
}

/// The pooling allocator is tailor made for the `wasmtime serve` use case, so
/// try to use it when we can. The main cost of the pooling allocator, however,
/// is the virtual memory required to run it. Not all systems support the same
/// amount of virtual memory, for example some aarch64 and riscv64 configuration
/// only support 39 bits of virtual address space.
///
/// The pooling allocator, by default, will request 1000 linear memories each
/// sized at 6G per linear memory. This is 6T of virtual memory which ends up
/// being about 42 bits of the address space. This exceeds the 39 bit limit of
/// some systems, so there the pooling allocator will fail by default.
///
/// This function attempts to dynamically determine the hint for the pooling
/// allocator. This returns `Some(true)` if the pooling allocator should be used
/// by default, or `None` or an error otherwise.
///
/// The method for testing this is to allocate a 0-sized 64-bit linear memory
/// with a maximum size that's N bits large where we force all memories to be
/// static. This should attempt to acquire N bits of the virtual address space.
/// If successful that should mean that the pooling allocator is OK to use, but
/// if it fails then the pooling allocator is not used and the normal mmap-based
/// implementation is used instead.
fn use_pooling_allocator_by_default() -> Result<Option<bool>> {
    use wasmtime::{Config, Memory, MemoryType};
    const BITS_TO_TEST: u32 = 42;
    let mut config = Config::new();
    config.wasm_memory64(true);
    config.memory_reservation(1 << BITS_TO_TEST);
    let engine = Engine::new(&config)?;
    let mut store = Store::new(&engine, ());
    // NB: the maximum size is in wasm pages to take out the 16-bits of wasm
    // page size here from the maximum size.
    let ty = MemoryType::new64(0, Some(1 << (BITS_TO_TEST - 16)));
    if Memory::new(&mut store, ty).is_ok() {
        Ok(Some(true))
    } else {
        Ok(None)
    }
}
