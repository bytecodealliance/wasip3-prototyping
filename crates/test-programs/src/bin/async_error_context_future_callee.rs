mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "error-context-future-callee",
        async: {
            exports: [
                "local:local/run#run",
                "local:local/run-future#produce-then-error",
            ],
        }
    });

    use super::Component;
    export!(Component);
}

use bindings::wit_future;
use wit_bindgen_rt::async_support::{self, error_context_new, FutureReader};

struct Component;

impl bindings::exports::local::local::run_future::Guest for Component {
    async fn produce_then_error() -> FutureReader<()> {
        let (mut tx, rx) = wit_future::new();
        async_support::spawn(async move {
            tx.close_with_error(error_context_new("error".into()));
        });
        rx
    }
}

impl bindings::exports::local::local::run::Guest for Component {
    async fn run() {}
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
