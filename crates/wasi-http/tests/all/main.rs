mod http_server;
mod p2;
mod p3;

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
