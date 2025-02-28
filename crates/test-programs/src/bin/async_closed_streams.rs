mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "closed-streams",
        async: true,
    });

    use super::Component;
    export!(Component);
}

use {
    bindings::exports::local::local::closed::Guest,
    wit_bindgen_rt::async_support::{futures::StreamExt, FutureReader, StreamReader},
};

struct Component;

impl Guest for Component {
    async fn read_stream(mut rx: StreamReader<u8>, expected: Vec<u8>) {
        assert_eq!(rx.next().await.unwrap().unwrap(), expected);
    }

    async fn read_future(rx: FutureReader<u8>, expected: u8, _rx_ignored: FutureReader<u8>) {
        assert_eq!(rx.await.unwrap().unwrap(), expected);
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
