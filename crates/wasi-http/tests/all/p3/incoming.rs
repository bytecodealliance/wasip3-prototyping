use bytes::Bytes;
use futures::try_join;
use http_body::Body;
use http_body_util::{BodyExt as _, Collected, Empty};
use wasmtime::Store;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::p3::cli::WasiCliCtx;
use wasmtime_wasi_http::p3::bindings::Proxy;
use wasmtime_wasi_http::p3::bindings::http::types::ErrorCode;
use wasmtime_wasi_http::p3::{Response, WasiHttpCtx};

use super::{Ctx, TestClient};

pub async fn run_wasi_http<E: Into<ErrorCode> + 'static>(
    component_filename: &str,
    req: http::Request<impl Body<Data = Bytes, Error = E> + Send + Sync + 'static>,
    client: TestClient,
) -> anyhow::Result<Result<http::Response<Collected<Bytes>>, Option<ErrorCode>>> {
    let engine = test_programs_artifacts::engine(|config| {
        config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        config.async_support(true);
        config.wasm_component_model_async(true);
    });
    let component = Component::from_file(&engine, component_filename)?;

    let mut store = Store::new(
        &engine,
        Ctx {
            cli: WasiCliCtx {
                ..WasiCliCtx::default()
            },
            http: WasiHttpCtx { client },
            ..Ctx::default()
        },
    );

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p3::add_to_linker(&mut linker)?;
    let instance = linker.instantiate_async(&mut store, &component).await?;
    let proxy = Proxy::new(&mut store, &instance)?;
    let res = match instance
        .run_with(&mut store, async |s| proxy.handle(s, req).await)
        .await??
    {
        Ok(res) => res,
        Err(err) => return Ok(Err(Some(err))),
    };
    let (res, io) = Response::resource_into_http(&mut store, res)?;
    let (parts, body) = res.into_parts();
    let (res, body) = try_join!(
        instance.run_with(&mut store, async |store| io
            .run(store, async { Ok(()) })
            .await),
        async { Ok(body.collect().await) },
    )?;
    res?;
    let body = match body {
        Ok(body) => body,
        Err(err) => return Ok(Err(err)),
    };
    Ok(Ok(http::Response::from_parts(parts, body)))
}

#[test_log::test(tokio::test)]
async fn wasi_http_proxy_tests() -> anyhow::Result<()> {
    let req = http::Request::builder()
        .uri("http://example.com:8080/test-path")
        .method(http::Method::GET);

    let resp = run_wasi_http(
        test_programs_artifacts::P3_API_PROXY_COMPONENT,
        req.body(Empty::new())?,
        TestClient::default(),
    )
    .await?;

    match resp {
        Ok(resp) => println!("response: {resp:?}"),
        Err(e) => panic!("Error given in response: {e:?}"),
    };

    Ok(())
}

// TODO: Port below
//
//#[test_log::test(tokio::test)]
//async fn wasi_http_hash_all() -> Result<()> {
//    do_wasi_http_hash_all(false).await
//}
//
//#[test_log::test(tokio::test)]
//async fn wasi_http_hash_all_with_override() -> Result<()> {
//    do_wasi_http_hash_all(true).await
//}
//
//async fn do_wasi_http_hash_all(override_send_request: bool) -> Result<()> {
//    let bodies = Arc::new(
//        [
//            ("/a", "’Twas brillig, and the slithy toves"),
//            ("/b", "Did gyre and gimble in the wabe:"),
//            ("/c", "All mimsy were the borogoves,"),
//            ("/d", "And the mome raths outgrabe."),
//        ]
//        .into_iter()
//        .collect::<HashMap<_, _>>(),
//    );
//
//    let listener = tokio::net::TcpListener::bind((Ipv4Addr::new(127, 0, 0, 1), 0)).await?;
//
//    let prefix = format!("http://{}", listener.local_addr()?);
//
//    let (_tx, rx) = oneshot::channel::<()>();
//
//    let handle = {
//        let bodies = bodies.clone();
//
//        move |request: http::request::Parts| {
//            if let (Method::GET, Some(body)) = (request.method, bodies.get(request.uri.path())) {
//                Ok::<_, anyhow::Error>(hyper::Response::new(body::full(Bytes::copy_from_slice(
//                    body.as_bytes(),
//                ))))
//            } else {
//                Ok(hyper::Response::builder()
//                    .status(StatusCode::METHOD_NOT_ALLOWED)
//                    .body(body::empty())?)
//            }
//        }
//    };
//
//    let send_request = if override_send_request {
//        Some(Arc::new(
//            move |request: hyper::Request<HyperOutgoingBody>,
//                  OutgoingRequestConfig {
//                      between_bytes_timeout,
//                      ..
//                  }| {
//                let response = handle(request.into_parts().0).map(|resp| {
//                    Ok(IncomingResponse {
//                        resp: resp.map(|body| {
//                            body.map_err(wasmtime_wasi_http::hyper_response_error)
//                                .boxed()
//                        }),
//                        worker: None,
//                        between_bytes_timeout,
//                    })
//                });
//                HostFutureIncomingResponse::ready(response)
//            },
//        ) as RequestSender)
//    } else {
//        let server = async move {
//            loop {
//                let (stream, _) = listener.accept().await?;
//                let stream = TokioIo::new(stream);
//                let handle = handle.clone();
//                task::spawn(async move {
//                    if let Err(e) = http1::Builder::new()
//                        .keep_alive(true)
//                        .serve_connection(
//                            stream,
//                            service_fn(move |request| {
//                                let handle = handle.clone();
//                                async move { handle(request.into_parts().0) }
//                            }),
//                        )
//                        .await
//                    {
//                        eprintln!("error serving connection: {e:?}");
//                    }
//                });
//
//                // Help rustc with type inference:
//                if false {
//                    return Ok::<_, anyhow::Error>(());
//                }
//            }
//        }
//        .then(|result| {
//            if let Err(e) = result {
//                eprintln!("error listening for connections: {e:?}");
//            }
//            future::ready(())
//        })
//        .boxed();
//
//        task::spawn(async move {
//            drop(future::select(server, rx).await);
//        });
//
//        None
//    };
//
//    let mut request = hyper::Request::builder()
//        .method(http::Method::GET)
//        .uri("http://example.com:8080/hash-all");
//    for path in bodies.keys() {
//        request = request.header("url", format!("{prefix}{path}"));
//    }
//    let request = request.body(body::empty())?;
//
//    let response = run_wasi_http(
//        test_programs_artifacts::API_PROXY_STREAMING_COMPONENT,
//        request,
//        send_request,
//        None,
//    )
//    .await??;
//
//    assert_eq!(StatusCode::OK, response.status());
//    let body = response.into_body().to_bytes();
//    let body = str::from_utf8(&body)?;
//    for line in body.lines() {
//        let (url, hash) = line
//            .split_once(": ")
//            .ok_or_else(|| anyhow!("expected string of form `<url>: <sha-256>`; got {line}"))?;
//
//        let path = url
//            .strip_prefix(&prefix)
//            .ok_or_else(|| anyhow!("expected string with prefix {prefix}; got {url}"))?;
//
//        let mut hasher = Sha256::new();
//        hasher.update(
//            bodies
//                .get(path)
//                .ok_or_else(|| anyhow!("unexpected path: {path}"))?,
//        );
//
//        use base64::Engine;
//        assert_eq!(
//            hash,
//            base64::engine::general_purpose::STANDARD_NO_PAD.encode(hasher.finalize())
//        );
//    }
//
//    Ok(())
//}
//
//// ensure the runtime rejects the outgoing request
//#[test_log::test(tokio::test)]
//async fn wasi_http_hash_all_with_reject() -> Result<()> {
//    let request = hyper::Request::builder()
//        .method(http::Method::GET)
//        .uri("http://example.com:8080/hash-all");
//    let request = request.header("url", format!("http://forbidden.com"));
//    let request = request.header("url", format!("http://localhost"));
//    let request = request.body(body::empty())?;
//
//    let response = run_wasi_http(
//        test_programs_artifacts::API_PROXY_STREAMING_COMPONENT,
//        request,
//        None,
//        Some("forbidden.com".to_string()),
//    )
//    .await??;
//
//    let body = response.into_body().to_bytes();
//    let body = str::from_utf8(&body).unwrap();
//    for line in body.lines() {
//        println!("{line}");
//        if line.contains("forbidden.com") {
//            assert!(line.contains("HttpRequestDenied"));
//        }
//        if line.contains("localhost") {
//            assert!(!line.contains("HttpRequestDenied"));
//        }
//    }
//
//    Ok(())
//}
//
//#[test_log::test(tokio::test)]
//async fn wasi_http_echo() -> Result<()> {
//    do_wasi_http_echo("echo", None).await
//}
//
//#[test_log::test(tokio::test)]
//async fn wasi_http_double_echo() -> Result<()> {
//    let listener = tokio::net::TcpListener::bind((Ipv4Addr::new(127, 0, 0, 1), 0)).await?;
//
//    let prefix = format!("http://{}", listener.local_addr()?);
//
//    let (_tx, rx) = oneshot::channel::<()>();
//
//    let server = async move {
//        loop {
//            let (stream, _) = listener.accept().await?;
//            let stream = TokioIo::new(stream);
//            task::spawn(async move {
//                if let Err(e) = http1::Builder::new()
//                    .keep_alive(true)
//                    .serve_connection(
//                        stream,
//                        service_fn(
//                            move |request: hyper::Request<hyper::body::Incoming>| async move {
//                                use http_body_util::BodyExt;
//
//                                if let (&Method::POST, "/echo") =
//                                    (request.method(), request.uri().path())
//                                {
//                                    Ok::<_, anyhow::Error>(hyper::Response::new(
//                                        request.into_body().boxed(),
//                                    ))
//                                } else {
//                                    Ok(hyper::Response::builder()
//                                        .status(StatusCode::METHOD_NOT_ALLOWED)
//                                        .body(BoxBody::new(
//                                            Empty::new().map_err(|_| unreachable!()),
//                                        ))?)
//                                }
//                            },
//                        ),
//                    )
//                    .await
//                {
//                    eprintln!("error serving connection: {e:?}");
//                }
//            });
//
//            // Help rustc with type inference:
//            if false {
//                return Ok::<_, anyhow::Error>(());
//            }
//        }
//    }
//    .then(|result| {
//        if let Err(e) = result {
//            eprintln!("error listening for connections: {e:?}");
//        }
//        future::ready(())
//    })
//    .boxed();
//
//    task::spawn(async move {
//        drop(future::select(server, rx).await);
//    });
//
//    do_wasi_http_echo("double-echo", Some(&format!("{prefix}/echo"))).await
//}
//
//async fn do_wasi_http_echo(uri: &str, url_header: Option<&str>) -> Result<()> {
//    let body = {
//        // A sorta-random-ish megabyte
//        let mut n = 0_u8;
//        iter::repeat_with(move || {
//            n = n.wrapping_add(251);
//            n
//        })
//        .take(1024 * 1024)
//        .collect::<Vec<_>>()
//    };
//
//    let mut request = hyper::Request::builder()
//        .method(http::Method::POST)
//        .uri(format!("http://example.com:8080/{uri}"))
//        .header("content-type", "application/octet-stream");
//
//    if let Some(url_header) = url_header {
//        request = request.header("url", url_header);
//    }
//
//    let request = request.body(BoxBody::new(StreamBody::new(stream::iter(
//        body.chunks(16 * 1024)
//            .map(|chunk| Ok::<_, hyper::Error>(Frame::data(Bytes::copy_from_slice(chunk))))
//            .collect::<Vec<_>>(),
//    ))))?;
//
//    let response = run_wasi_http(
//        test_programs_artifacts::API_PROXY_STREAMING_COMPONENT,
//        request,
//        None,
//        None,
//    )
//    .await??;
//
//    assert_eq!(StatusCode::OK, response.status());
//    assert_eq!(
//        response.headers()["content-type"],
//        "application/octet-stream"
//    );
//    let received = Vec::from(response.into_body().to_bytes());
//    if body != received {
//        panic!(
//            "body content mismatch (expected length {}; actual length {})",
//            body.len(),
//            received.len()
//        );
//    }
//
//    Ok(())
//}
//
//#[test_log::test(tokio::test)]
//async fn wasi_http_without_port() -> Result<()> {
//    let req = hyper::Request::builder()
//        .method(http::Method::GET)
//        .uri("https://httpbin.org/get");
//
//    let _response: hyper::Response<_> = run_wasi_http(
//        test_programs_artifacts::API_PROXY_FORWARD_REQUEST_COMPONENT,
//        req.body(body::empty())?,
//        None,
//        None,
//    )
//    .await??;
//
//    // NB: don't test the actual return code of `response`. This is testing a
//    // live http request against a live server and things happen. If we got this
//    // far it's already successful that the request was made and the lack of
//    // port in the URI was handled.
//
//    Ok(())
//}
