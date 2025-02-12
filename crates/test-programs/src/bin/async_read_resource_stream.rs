mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "read-resource-stream",
        async: {
            exports: [
                "local:local/run#run"
            ]
        }
    });

    use super::Component;
    export!(Component);
}

use {
    bindings::{exports::local::local::run::Guest, local::local::resource_stream},
    futures::StreamExt,
};

struct Component;

impl Guest for Component {
    async fn run() {
        let mut count = 7;
        let mut stream = resource_stream::foo(count);

        while let Some(Ok(chunk)) = stream.next().await {
            for x in chunk {
                if count > 0 {
                    count -= 1;
                } else {
                    panic!("received too many items");
                }

                x.foo()
            }
        }

        if count != 0 {
            panic!("received too few items");
        }
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
