pub mod sockets;

wit_bindgen::generate!({
    path: "../wasi/src/p3/wit",
    world: "wasi:cli/command",
    default_bindings_module: "test_programs::p3",
    pub_export_macro: true,
    async: {
         imports: [
             "wasi:clocks/monotonic-clock@0.3.0#wait-for",
             "wasi:clocks/monotonic-clock@0.3.0#wait-until",
             "wasi:sockets/ip-name-lookup@0.3.0#resolve-addresses",
             "wasi:sockets/types@0.3.0#[method]tcp-socket.connect",
             "wasi:sockets/types@0.3.0#[method]tcp-socket.send",
         ],
         exports: [
             "wasi:cli/run@0.3.0#run",
         ],
    },
    generate_all,
});
