use super::*;
use test_programs_artifacts::*;
use wasmtime_wasi::p2::bindings::sync::Command;

foreach_http!(assert_test_exists);

fn run(path: &str, server: &Server) -> Result<()> {
    let engine = test_programs_artifacts::engine(|config| {
        config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
    });
    let component = Component::from_file(&engine, path)?;
    let mut store = store(&engine, server);
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    wasmtime_wasi_http::add_only_http_to_linker_sync(&mut linker)?;
    let command = Command::instantiate(&mut store, &component, &linker)?;
    let result = command.wasi_cli_run().call_run(&mut store)?;
    result.map_err(|()| anyhow::anyhow!("run returned an error"))
}

#[test_log::test]
fn http_outbound_request_get() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_GET_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_timeout() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_TIMEOUT_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_post() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_POST_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_large_post() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_LARGE_POST_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_put() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_PUT_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_invalid_version() -> Result<()> {
    let server = Server::http2(1)?;
    run(HTTP_OUTBOUND_REQUEST_INVALID_VERSION_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_invalid_header() -> Result<()> {
    let server = Server::http2(1)?;
    run(HTTP_OUTBOUND_REQUEST_INVALID_HEADER_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_unknown_method() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_UNKNOWN_METHOD_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_unsupported_scheme() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_UNSUPPORTED_SCHEME_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_invalid_port() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_INVALID_PORT_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_invalid_dnsname() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_INVALID_DNSNAME_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_response_build() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_RESPONSE_BUILD_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_content_length() -> Result<()> {
    let server = Server::http1(1)?;
    run(HTTP_OUTBOUND_REQUEST_CONTENT_LENGTH_COMPONENT, &server)
}

#[test_log::test]
fn http_outbound_request_missing_path_and_query() -> Result<()> {
    let server = Server::http1(1)?;
    run(
        HTTP_OUTBOUND_REQUEST_MISSING_PATH_AND_QUERY_COMPONENT,
        &server,
    )
}
