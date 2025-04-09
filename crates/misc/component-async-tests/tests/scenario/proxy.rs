use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use bytes::{Bytes, BytesMut};
use component_async_tests::Ctx;
use tokio::fs;
use wasi_http_draft::wasi::http::types::{ErrorCode, Method, Scheme};
use wasi_http_draft::{Body, Fields, Request, Response};
use wasmtime::component::{
    Accessor, Component, Linker, PromisesUnordered, Resource, ResourceTable, StreamReader,
    StreamWriter,
};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{IoView, WasiCtxBuilder};

use component_async_tests::util::{annotate, compose, init_logger};

mod sleep {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "sleep-host",
        concurrent_imports: true,
        concurrent_exports: true,
        async: {
            only_imports: [
                "local:local/sleep#sleep-millis",
            ]
        },
    });
}

impl sleep::local::local::sleep::Host for &mut Ctx {
    async fn sleep_millis<T>(_: &mut Accessor<T, Self>, time_in_millis: u64) {
        tokio::time::sleep(Duration::from_millis(time_in_millis)).await;
    }
}

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
            write::{DeflateDecoder, DeflateEncoder},
            Compression,
        },
        std::io::Write,
    };

    init_logger();

    let mut config = Config::new();
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);

    let engine = Engine::new(&config)?;

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasi_http_draft::add_to_linker(&mut linker)?;
    sleep::local::local::sleep::add_to_linker_get_host(&mut linker, annotate(|ctx| ctx))?;

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

    let mut promises = PromisesUnordered::new();

    let (request_body_tx, request_body_rx) = instance.stream::<_, _, BytesMut, _, _>(&mut store)?;

    promises.push(
        request_body_tx
            .write_all(if use_compression {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                encoder.write_all(body)?;
                Cursor::new(Bytes::from(encoder.finish()?))
            } else {
                Cursor::new(Bytes::copy_from_slice(body))
            })
            .map(|(w, _)| Event::RequestBodyWrite(w)),
    );

    let trailers = vec![("fizz".into(), b"buzz".into())];

    let (request_trailers_tx, request_trailers_rx) = instance.future(&mut store)?;

    let request_trailers = IoView::table(store.data_mut()).push(Fields(trailers.clone()))?;

    promises.push(
        request_trailers_tx
            .write(request_trailers)
            .map(Event::RequestTrailersWrite),
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

    promises.push(
        proxy
            .wasi_http_handler()
            .call_handle(&mut store, request)
            .await?
            .map(Event::Response),
    );

    let mut response_body = Vec::new();
    let mut response_trailers = None;
    let mut received_trailers = false;
    while let Some(event) = promises.next(&mut store).await? {
        match event {
            Event::RequestBodyWrite(Some(_)) => {}
            Event::RequestBodyWrite(None) => panic!("write should have been accepted"),
            Event::RequestTrailersWrite(success) => assert!(success),
            Event::Response(response) => {
                let mut response = IoView::table(store.data_mut()).delete(response?)?;

                assert!(response.status_code == 200);

                assert!(headers.iter().all(|(k0, v0)| response
                    .headers
                    .0
                    .iter()
                    .any(|(k1, v1)| k0 == k1 && v0 == v1)));

                if use_compression {
                    assert!(response.headers.0.iter().any(|(k, v)| matches!(
                        (k.as_str(), v.as_slice()),
                        ("content-encoding", b"deflate")
                    )));
                    assert!(response
                        .headers
                        .0
                        .iter()
                        .all(|(k, _)| k.as_str() != "content-length"));
                }

                response_trailers = response.body.trailers.take();

                promises.push(
                    response
                        .body
                        .stream
                        .take()
                        .unwrap()
                        .read(BytesMut::with_capacity(8096))
                        .map(|(r, b)| Event::ResponseBodyRead(r, b)),
                );
            }
            Event::ResponseBodyRead(Some(rx), mut buffer) => {
                response_body.extend(&buffer);
                buffer.clear();
                promises.push(rx.read(buffer).map(|(r, b)| Event::ResponseBodyRead(r, b)));
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

                promises.push(
                    response_trailers
                        .take()
                        .unwrap()
                        .read()
                        .map(Event::ResponseTrailersRead),
                );
            }
            Event::ResponseTrailersRead(Some(response_trailers)) => {
                let response_trailers =
                    IoView::table(store.data_mut()).delete(response_trailers)?;

                assert!(trailers.iter().all(|(k0, v0)| response_trailers
                    .0
                    .iter()
                    .any(|(k1, v1)| k0 == k1 && v0 == v1)));

                received_trailers = true;
            }
            Event::ResponseTrailersRead(None) => panic!("expected response trailers; got none"),
        }
    }

    assert!(received_trailers);

    Ok(())
}
