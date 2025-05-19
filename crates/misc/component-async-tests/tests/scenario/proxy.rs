use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use bytes::{Bytes, BytesMut};
use component_async_tests::{Ctx, sleep};
use futures::{
    FutureExt,
    stream::{FuturesUnordered, TryStreamExt},
};
use tokio::fs;
use wasi_http_draft::wasi::http::types::{ErrorCode, Method, Scheme};
use wasi_http_draft::{Body, Fields, Request, Response};
use wasmtime::component::{Component, Linker, Resource, ResourceTable, StreamReader, StreamWriter};
use wasmtime::{Engine, Store};
use wasmtime_wasi::p2::{IoView, WasiCtxBuilder};

use component_async_tests::util::{compose, config, init_logger};

#[tokio::test]
pub async fn async_http_echo() -> Result<()> {
    test_http_echo(
        &fs::read(test_programs_artifacts::ASYNC_HTTP_ECHO_COMPONENT).await?,
        false,
    )
    .await
}

#[tokio::test]
pub async fn async_http_middleware() -> Result<()> {
    let echo = &fs::read(test_programs_artifacts::ASYNC_HTTP_ECHO_COMPONENT).await?;
    let middleware = &fs::read(test_programs_artifacts::ASYNC_HTTP_MIDDLEWARE_COMPONENT).await?;
    test_http_echo(&compose(middleware, echo).await?, true).await
}

#[tokio::test]
pub async fn async_http_middleware_with_chain() -> Result<()> {
    use wasm_compose::{
        composer::ComponentComposer,
        config::{Config, Dependency, Instantiation, InstantiationArg},
    };

    let composed = &{
        let dir = tempfile::tempdir()?;

        fs::copy(
            test_programs_artifacts::ASYNC_HTTP_ECHO_COMPONENT,
            &dir.path().join("chain-http.wasm"),
        )
        .await?;

        ComponentComposer::new(
            Path::new(test_programs_artifacts::ASYNC_HTTP_MIDDLEWARE_WITH_CHAIN_COMPONENT),
            &Config {
                dir: dir.path().to_owned(),
                definitions: Vec::new(),
                search_paths: Vec::new(),
                skip_validation: false,
                import_components: false,
                disallow_imports: false,
                dependencies: [(
                    "local:local/chain-http".to_owned(),
                    Dependency {
                        path: test_programs_artifacts::ASYNC_HTTP_ECHO_COMPONENT.into(),
                    },
                )]
                .into_iter()
                .collect(),
                instantiations: [(
                    "root".to_owned(),
                    Instantiation {
                        dependency: Some("local:local/chain-http".to_owned()),
                        arguments: [(
                            "local:local/chain-http".to_owned(),
                            InstantiationArg {
                                instance: "local:local/chain-http".into(),
                                export: Some("wasi:http/handler@0.3.0-draft".into()),
                            },
                        )]
                        .into_iter()
                        .collect(),
                    },
                )]
                .into_iter()
                .collect(),
            },
        )
        .compose()?
    };

    test_http_echo(composed, true).await
}

async fn test_http_echo(component: &[u8], use_compression: bool) -> Result<()> {
    use {
        flate2::{
            Compression,
            write::{DeflateDecoder, DeflateEncoder},
        },
        std::io::Write,
    };

    init_logger();

    let engine = Engine::new(&config())?;

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasi_http_draft::add_to_linker(&mut linker)?;
    sleep::local::local::sleep::add_to_linker::<_, Ctx>(&mut linker, |ctx| ctx)?;

    let mut store = Store::new(
        &engine,
        Ctx {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::default(),
            continue_: false,
            wakers: Arc::new(Mutex::new(None)),
        },
    );

    let instance = linker.instantiate_async(&mut store, &component).await?;

    let proxy = component_async_tests::proxy::bindings::Proxy::new(&mut store, &instance)?;

    let headers = [("foo".into(), b"bar".into())];

    let body = b"And the mome raths outgrabe";

    enum Event {
        RequestBodyWrite(Option<StreamWriter<Cursor<Bytes>>>),
        RequestTrailersWrite(bool),
        Response(Result<Resource<Response>, ErrorCode>),
        ResponseBodyRead(Option<StreamReader<BytesMut>>, BytesMut),
        ResponseTrailersRead(Option<Resource<Fields>>),
    }

    let mut futures = FuturesUnordered::new();

    let (request_body_tx, request_body_rx) = instance.stream::<_, _, BytesMut, _, _>(&mut store)?;

    futures.push(
        request_body_tx
            .write_all(if use_compression {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                encoder.write_all(body)?;
                Cursor::new(Bytes::from(encoder.finish()?))
            } else {
                Cursor::new(Bytes::copy_from_slice(body))
            })
            .map(|(w, _)| Ok(Event::RequestBodyWrite(w)))
            .boxed(),
    );

    let trailers = vec![("fizz".into(), b"buzz".into())];

    let (request_trailers_tx, request_trailers_rx) = instance.future(&mut store)?;

    let request_trailers = IoView::table(store.data_mut()).push(Fields(trailers.clone()))?;

    futures.push(
        request_trailers_tx
            .write(request_trailers)
            .map(Event::RequestTrailersWrite)
            .map(Ok)
            .boxed(),
    );

    let request = IoView::table(store.data_mut()).push(Request {
        method: Method::Post,
        scheme: Some(Scheme::Http),
        path_with_query: Some("/".into()),
        authority: Some("localhost".into()),
        headers: Fields(
            headers
                .iter()
                .cloned()
                .chain(if use_compression {
                    vec![
                        ("content-encoding".into(), b"deflate".into()),
                        (
                            "accept-encoding".into(),
                            b"nonexistent-encoding, deflate".into(),
                        ),
                    ]
                } else {
                    Vec::new()
                })
                .collect(),
        ),
        body: Body {
            stream: Some(request_body_rx),
            trailers: Some(request_trailers_rx),
        },
        options: None,
    })?;

    futures.push(
        proxy
            .wasi_http_handler()
            .call_handle(&mut store, request)
            .map(|v| v.map(Event::Response))
            .boxed(),
    );

    let mut response_body = Vec::new();
    let mut response_trailers = None;
    let mut received_trailers = false;
    while let Some(event) = instance.run(&mut store, futures.try_next()).await?? {
        match event {
            Event::RequestBodyWrite(Some(_)) => {}
            Event::RequestBodyWrite(None) => panic!("write should have been accepted"),
            Event::RequestTrailersWrite(success) => assert!(success),
            Event::Response(response) => {
                let mut response = IoView::table(store.data_mut()).delete(response?)?;

                assert!(response.status_code == 200);

                assert!(headers.iter().all(|(k0, v0)| {
                    response
                        .headers
                        .0
                        .iter()
                        .any(|(k1, v1)| k0 == k1 && v0 == v1)
                }));

                if use_compression {
                    assert!(response.headers.0.iter().any(|(k, v)| matches!(
                        (k.as_str(), v.as_slice()),
                        ("content-encoding", b"deflate")
                    )));
                    assert!(
                        response
                            .headers
                            .0
                            .iter()
                            .all(|(k, _)| k.as_str() != "content-length")
                    );
                }

                response_trailers = response.body.trailers.take();

                futures.push(
                    response
                        .body
                        .stream
                        .take()
                        .unwrap()
                        .read(BytesMut::with_capacity(8096))
                        .map(|(r, b)| Ok(Event::ResponseBodyRead(r, b)))
                        .boxed(),
                );
            }
            Event::ResponseBodyRead(Some(rx), mut buffer) => {
                response_body.extend(&buffer);
                buffer.clear();
                futures.push(
                    rx.read(buffer)
                        .map(|(r, b)| Ok(Event::ResponseBodyRead(r, b)))
                        .boxed(),
                );
            }
            Event::ResponseBodyRead(None, _) => {
                let response_body = if use_compression {
                    let mut decoder = DeflateDecoder::new(Vec::new());
                    decoder.write_all(&response_body)?;
                    decoder.finish()?
                } else {
                    response_body.clone()
                };

                assert_eq!(body as &[_], &response_body);

                futures.push(
                    response_trailers
                        .take()
                        .unwrap()
                        .read()
                        .map(Event::ResponseTrailersRead)
                        .map(Ok)
                        .boxed(),
                );
            }
            Event::ResponseTrailersRead(Some(response_trailers)) => {
                let response_trailers =
                    IoView::table(store.data_mut()).delete(response_trailers)?;

                assert!(trailers.iter().all(|(k0, v0)| {
                    response_trailers
                        .0
                        .iter()
                        .any(|(k1, v1)| k0 == k1 && v0 == v1)
                }));

                received_trailers = true;
            }
            Event::ResponseTrailersRead(None) => panic!("expected response trailers; got none"),
        }
    }

    assert!(received_trailers);

    Ok(())
}
