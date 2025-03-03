use futures::SinkExt as _;
use test_programs::p3::wasi::http::types::{ErrorCode, Headers, Method, Request, Scheme, Trailers};
use test_programs::p3::{wit_future, wit_stream};
use wit_bindgen_rt::async_support::{FutureWriter, StreamWriter};

struct Component;

test_programs::p3::export!(Component);

fn make_request() -> (
    Request,
    StreamWriter<u8>,
    FutureWriter<Result<Option<Trailers>, ErrorCode>>,
) {
    let (contents_tx, contents_rx) = wit_stream::new();
    let (trailers_tx, trailers_rx) = wit_future::new();
    let (request, _) = Request::new(
        Headers::from_list(&[("Content-Length".to_string(), b"11".to_vec())]).unwrap(),
        Some(contents_rx),
        trailers_rx,
        None,
    );

    request.set_method(&Method::Post).expect("setting method");
    request
        .set_scheme(Some(&Scheme::Http))
        .expect("setting scheme");
    let addr = test_programs::p3::wasi::cli::environment::get_environment()
        .into_iter()
        .find_map(|(k, v)| k.eq("HTTP_SERVER").then_some(v))
        .unwrap();
    request
        .set_authority(Some(&addr))
        .expect("setting authority");
    request
        .set_path_with_query(Some("/"))
        .expect("setting path with query");

    (request, contents_tx, trailers_tx)
}

impl test_programs::p3::exports::wasi::cli::run::Guest for Component {
    async fn run() -> Result<(), ()> {
        {
            println!("writing enough");
            let (_, mut contents_tx, trailers_tx) = make_request();
            contents_tx.send(b"long enough".to_vec()).await.unwrap();
            drop(contents_tx);
            trailers_tx.write(Ok(None)).await;
        }

        {
            println!("writing too little");
            let (_, mut contents_tx, trailers_tx) = make_request();
            contents_tx.send(b"msg".to_vec()).await.unwrap();
            drop(contents_tx);
            trailers_tx.write(Ok(None)).await;

            // handle()

            // TODO: Figure out how/if to represent this in wasip3
            //let e = OutgoingBody::finish(outgoing_body, None)
            //    .expect_err("finish should fail");

            //assert!(
            //    matches!(&e, ErrorCode::HttpRequestBodySize(Some(3))),
            //    "unexpected error: {e:#?}"
            //);
        }

        {
            println!("writing too much");
            let (_, mut contents_tx, trailers_tx) = make_request();
            contents_tx
                .send(b"more than 11 bytes".to_vec())
                .await
                .unwrap();
            drop(contents_tx);
            trailers_tx.write(Ok(None)).await;

            // TODO: Figure out how/if to represent this in wasip3
            //let e = request_body
            //    .blocking_write_and_flush("more than 11 bytes".as_bytes())
            //    .expect_err("write should fail");
            //let e = match e {
            //    test_programs::wasi::io::streams::StreamError::LastOperationFailed(e) => {
            //        http_error_code(&e)
            //    }
            //    test_programs::wasi::io::streams::StreamError::Closed => panic!("request closed"),
            //};
            //assert!(
            //    matches!(
            //        e,
            //        Some(ErrorCode::HttpRequestBodySize(Some(18)))
            //    ),
            //    "unexpected error {e:?}"
            //);
            //let e = OutgoingBody::finish(outgoing_body, None)
            //    .expect_err("finish should fail");

            //assert!(
            //    matches!(&e, ErrorCode::HttpRequestBodySize(Some(18))),
            //    "unexpected error: {e:#?}"
            //);
        }
        Ok(())
    }
}

fn main() {}
