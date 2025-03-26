struct Component;

test_programs::p3::export!(Component);

impl test_programs::p3::exports::wasi::cli::run::Guest for Component {
    async fn run() -> Result<(), ()> {
        let (_, rx) = test_programs::p3::wit_future::new();
        test_programs::p3::wasi::http::types::Request::new(
            test_programs::p3::wasi::http::types::Fields::new(),
            None,
            rx,
            None,
        );
        eprintln!("created request");
        Ok(())
    }
}

fn main() {}
