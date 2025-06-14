use {
    test_programs::p3::{
        proxy::exports::wasi::http::handler::Guest as Handler,
        wasi::http::types::{ErrorCode, Request, Response},
        wit_future, wit_stream,
    },
    wit_bindgen_rt::async_support::{self, StreamResult},
};

struct Component;

test_programs::p3::proxy::export!(Component);

impl Handler for Component {
    /// Return a response which echoes the request headers, body, and trailers.
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        let headers = request.headers();
        let (body, trailers) = request.body().unwrap();

        // let (headers, body) = Request::into_parts(request);

        let (response, _result) = if false {
            // This is the easy and efficient way to do it...
            Response::new(headers, Some(body), trailers)
        } else {
            // ...but we do it the more difficult, less efficient way here to exercise various component model
            // features (e.g. `future`s, `stream`s, and post-return asynchronous execution):
            let (trailers_tx, trailers_rx) = wit_future::new(|| todo!());
            let (mut pipe_tx, pipe_rx) = wit_stream::new();

            async_support::spawn(async move {
                let mut body_rx = body;
                let mut chunk = Vec::with_capacity(1024);
                loop {
                    let (status, buf) = body_rx.read(chunk).await;
                    chunk = buf;
                    match status {
                        StreamResult::Complete(_) => {
                            chunk = pipe_tx.write_all(chunk).await;
                            assert!(chunk.is_empty());
                        }
                        StreamResult::Dropped => break,
                        StreamResult::Cancelled => unreachable!(),
                    }
                }

                drop(pipe_tx);

                trailers_tx.write(trailers.await).await.unwrap();
                drop(request);
            });

            Response::new(headers, Some(pipe_rx), trailers_rx)
        };

        Ok(response)
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
