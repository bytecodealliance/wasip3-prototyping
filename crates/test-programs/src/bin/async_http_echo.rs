mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "wasi:http/proxy",
    });

    use super::Component;
    export!(Component);
}

use {
    bindings::{
        exports::wasi::http::handler::Guest as Handler,
        wasi::http::types::{Body, ErrorCode, Request, Response},
        wit_future, wit_stream,
    },
    wit_bindgen_rt::async_support::{self, StreamResult},
};

struct Component;

impl Handler for Component {
    /// Return a response which echoes the request headers, body, and trailers.
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        let (headers, body) = Request::into_parts(request);

        if false {
            // This is the easy and efficient way to do it...
            Ok(Response::new(headers, body))
        } else {
            // ...but we do it the more difficult, less efficient way here to exercise various component model
            // features (e.g. `future`s, `stream`s, and post-return asynchronous execution):
            let (trailers_tx, trailers_rx) = wit_future::new(|| todo!());
            let (mut pipe_tx, pipe_rx) = wit_stream::new();

            async_support::spawn(async move {
                let mut body_rx = body.stream().unwrap();
                let mut chunk = Vec::with_capacity(1024);
                loop {
                    let (status, buf) = body_rx.read(chunk).await;
                    chunk = buf;
                    match status {
                        StreamResult::Complete(_) => {
                            chunk = pipe_tx.write_all(chunk).await;
                            assert!(chunk.is_empty());
                        }
                        StreamResult::Closed => break,
                        StreamResult::Cancelled => unreachable!(),
                    }
                }

                drop(pipe_tx);

                if let Some(trailers) = Body::finish(body) {
                    trailers_tx.write(trailers.await).await.unwrap();
                }
            });

            Ok(Response::new(
                headers,
                Body::new_with_trailers(pipe_rx, trailers_rx),
            ))
        }
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
