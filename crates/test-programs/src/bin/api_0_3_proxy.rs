use futures::{join, SinkExt as _};
use test_programs::p3::wasi::http::types::{ErrorCode, Headers, Request, Response};
use test_programs::p3::{wit_future, wit_stream};
use wit_bindgen_rt::async_support::spawn;

struct T;

test_programs::p3::proxy::export!(T);

impl test_programs::p3::proxy::exports::wasi::http::handler::Guest for T {
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        assert!(request.scheme().is_some());
        assert!(request.authority().is_some());
        assert!(request.path_with_query().is_some());

        // TODO: adapt below
        //test_filesystem();

        let header = String::from("custom-forbidden-header");
        let req_hdrs = request.headers();

        assert!(
            !req_hdrs.has(&header),
            "forbidden `custom-forbidden-header` found in request"
        );

        assert!(req_hdrs.delete(&header).is_err());
        assert!(req_hdrs.append(&header, b"no".as_ref()).is_err());

        assert!(
            !req_hdrs.has(&header),
            "append of forbidden header succeeded"
        );

        let hdrs = Headers::new();
        let (mut contents_tx, contents_rx) = wit_stream::new();
        let (trailers_tx, trailers_rx) = wit_future::new();
        let (resp, transmit) = Response::new(hdrs, Some(contents_rx), trailers_rx);
        spawn(async {
            join!(
                async {
                    contents_tx
                        .send(b"hello, world!".to_vec())
                        .await
                        .expect("writing response");
                    drop(contents_tx);
                    trailers_tx.write(Ok(None));
                },
                async {
                    transmit
                        .await
                        .expect("failed to transmit response")
                        .unwrap()
                        .unwrap()
                }
            );
        });
        Ok(resp)
    }
}

// Technically this should not be here for a proxy, but given the current
// framework for tests it's required since this file is built as a `bin`
fn main() {}

// TODO: adapt below
//fn test_filesystem() {
//    assert!(std::fs::File::open(".").is_err());
//}
