pub mod http;
pub mod sockets;

wit_bindgen::generate!({
    inline: "
        package wasmtime:test;

        world testp3 {
            include wasi:cli/imports@0.3.0;
            include wasi:http/imports@0.3.0-draft;

            export wasi:cli/run@0.3.0;
        }
    ",
    path: "../wasi-http/src/p3/wit",
    world: "wasmtime:test/testp3",
    default_bindings_module: "test_programs::p3",
    pub_export_macro: true,
    async: {
         imports: [
             "wasi:clocks/monotonic-clock@0.3.0#wait-for",
             "wasi:clocks/monotonic-clock@0.3.0#wait-until",
             "wasi:filesystem/types@0.3.0#[method]descriptor.write-via-stream",
             "wasi:http/handler@0.3.0-draft#handle",
             "wasi:sockets/ip-name-lookup@0.3.0#resolve-addresses",
             "wasi:sockets/types@0.3.0#[method]tcp-socket.connect",
             "wasi:sockets/types@0.3.0#[method]tcp-socket.send",
             "wasi:sockets/types@0.3.0#[method]udp-socket.receive",
             "wasi:sockets/types@0.3.0#[method]udp-socket.send",
         ],
         exports: [
             "wasi:cli/run@0.3.0#run",
         ],
    },
    generate_all
});

pub mod proxy {
    wit_bindgen::generate!({
        inline: "
            package wasmtime:test;

            world proxyp3 {
                include wasi:http/proxy@0.3.0-draft;
            }
        ",
        path: "../wasi-http/src/p3/wit",
        world: "wasmtime:test/proxyp3",
        default_bindings_module: "test_programs::p3::proxy",
        pub_export_macro: true,
        async: {
             imports: [
                 "wasi:http/handler@0.3.0-draft#handle",
             ],
             exports: [
                 "wasi:http/handler@0.3.0-draft#handle",
             ],
        },
        with: {
            "wasi:http/handler@0.3.0-draft": generate,
            "wasi:http/types@0.3.0-draft": crate::p3::wasi::http::types,
            "wasi:random/random@0.3.0": crate::p3::wasi::random::random,
            "wasi:cli/stdout@0.3.0": crate::p3::wasi::cli::stdout,
            "wasi:cli/stderr@0.3.0": crate::p3::wasi::cli::stderr,
            "wasi:cli/stdin@0.3.0": crate::p3::wasi::cli::stdin,
            "wasi:clocks/monotonic-clock@0.3.0": crate::p3::wasi::clocks::monotonic_clock,
            "wasi:clocks/wall-clock@0.3.0": crate::p3::wasi::clocks::wall_clock,
        },
    });
}
