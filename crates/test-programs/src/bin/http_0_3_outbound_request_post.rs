use anyhow::Context;
use test_programs::p3::wasi::http::types::{Method, Scheme};

struct Component;

test_programs::p3::export!(Component);

impl test_programs::p3::exports::wasi::cli::run::Guest for Component {
    async fn run() -> Result<(), ()> {
        let addr = test_programs::p3::wasi::cli::environment::get_environment()
            .into_iter()
            .find_map(|(k, v)| k.eq("HTTP_SERVER").then_some(v))
            .unwrap();
        let res = test_programs::p3::http::request(
            Method::Post,
            Scheme::Http,
            &addr,
            "/post",
            Some(b"{\"foo\": \"bar\"}"),
            None,
            None,
            None,
            None,
        )
        .await
        .context("/post")
        .unwrap();

        println!("/post: {res:?}");
        assert_eq!(res.status, 200);
        let method = res.header("x-wasmtime-test-method").unwrap();
        assert_eq!(std::str::from_utf8(method).unwrap(), "POST");
        let uri = res.header("x-wasmtime-test-uri").unwrap();
        assert_eq!(std::str::from_utf8(uri).unwrap(), format!("/post"));
        assert_eq!(res.body, b"{\"foo\": \"bar\"}", "invalid body returned");
        Ok(())
    }
}

fn main() {}
