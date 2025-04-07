mod http_server;
mod p2;
mod p3;

// Assert that each of all test scenarios are testing everything through
// assertion of the existence of the test function itself.
#[macro_export]
macro_rules! assert_test_exists {
    ($name:ident) => {
        #[expect(unused_imports, reason = "only here to ensure a name exists")]
        use self::$name as _;
    };
}

mod body {
    use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
    use hyper::body::Bytes;
    use hyper::Error;

    pub fn full(bytes: Bytes) -> BoxBody<Bytes, Error> {
        BoxBody::new(Full::new(bytes).map_err(|_| unreachable!()))
    }

    pub fn empty() -> BoxBody<Bytes, Error> {
        BoxBody::new(Empty::new().map_err(|_| unreachable!()))
    }
}
