mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "error-context-future-caller",
        async: {
            imports: [
                "local:local/run-future#run-error",
            ],
            exports: [
                "local:local/run#run",
            ],
        }
    });

    use super::Component;
    export!(Component);
}
use std::future::IntoFuture;

use bindings::exports::local::local::run::Guest;

struct Component;

impl Guest for Component {
    async fn run() {
        let future = bindings::local::local::run_future::produce_then_error();
        let Some(Err(e)) = future.into_future().await else {
            panic!("missing expected error");
        };
        let _ = e.debug_message();
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
