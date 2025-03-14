mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "middleware-with-chain",
        async: {
            imports: [
                "local:local/chain-http#handle",
                "local:local/sleep#sleep-millis"
            ],
            exports: [
                "wasi:http/handler@0.3.0-draft#handle",
            ]
        },
        generate_all,
    });

    use super::Component;
    export!(Component);
}

use bindings::{
    exports::wasi::http::handler::Guest as Handler,
    local::local::{chain_http, sleep},
    wasi::http::types::{ErrorCode, Request, Response},
};

struct Component;

impl Handler for Component {
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        // First, sleep briefly.  This will ensure the next call happens via a
        // host->guest call to the `wit_bindgen_rt::async_support::callback`
        // function, which exercises different code paths in both the host and
        // the guest, which we want to test here.
        sleep::sleep_millis(10).await;

        chain_http::handle(request).await
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
