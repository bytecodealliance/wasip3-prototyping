use {
    anyhow::Result,
    component_async_tests::{closed_streams, Ctx},
    futures::{future, FutureExt},
    std::sync::{Arc, Mutex},
    tokio::fs,
    wasmtime::{
        component::{
            self, Component, ErrorContext, Linker, Promise, PromisesUnordered, ResourceTable,
            StreamReader, StreamWriter,
        },
        Config, Engine, Store,
    },
    wasmtime_wasi::WasiCtxBuilder,
};

#[tokio::test]
pub async fn async_watch_streams() -> Result<()> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);

    let engine = Engine::new(&config)?;

    let mut store = Store::new(&engine, ());

    // Test watching and then dropping the read end of a stream.
    let (tx, rx) = component::stream::<u8, Vec<u8>, _, _>(&mut store)?;
    let watch = tx.watch_reader();
    drop(rx);
    component::get(&mut store, watch).await?;

    // Test dropping and then watching the read end of a stream.
    let (tx, rx) = component::stream::<u8, Vec<u8>, _, _>(&mut store)?;
    drop(rx);
    component::get(&mut store, tx.watch_reader()).await?;

    // Test watching and then dropping the write end of a stream.
    let (tx, rx) = component::stream::<u8, Vec<u8>, _, _>(&mut store)?;
    let watch = rx.watch_writer();
    drop(tx);
    component::get(&mut store, watch).await?;

    // Test dropping and then watching the write end of a stream.
    let (tx, rx) = component::stream::<u8, Vec<u8>, _, _>(&mut store)?;
    drop(tx);
    component::get(&mut store, rx.watch_writer()).await?;

    // Test watching and then dropping the read end of a future.
    let (tx, rx) = component::future::<u8, _, _>(&mut store)?;
    let watch = tx.watch_reader();
    drop(rx);
    component::get(&mut store, watch).await?;

    // Test dropping and then watching the read end of a future.
    let (tx, rx) = component::future::<u8, _, _>(&mut store)?;
    drop(rx);
    component::get(&mut store, tx.watch_reader()).await?;

    // Test watching and then dropping the write end of a future.
    let (tx, rx) = component::future::<u8, _, _>(&mut store)?;
    let watch = rx.watch_writer();
    drop(tx);
    component::get(&mut store, watch).await?;

    // Test dropping and then watching the write end of a future.
    let (tx, rx) = component::future::<u8, _, _>(&mut store)?;
    drop(tx);
    component::get(&mut store, rx.watch_writer()).await?;

    #[allow(clippy::type_complexity)]
    enum Event {
        Write(Option<StreamWriter<Vec<u8>>>),
        Read(Result<(StreamReader<Vec<u8>>, Vec<u8>), Option<ErrorContext>>),
    }

    // Test watching, then writing to, then dropping, then writing again to the
    // read end of a stream.
    let mut promises = PromisesUnordered::new();
    let (tx, rx) = component::stream::<u8, Vec<u8>, _, _>(&mut store)?;
    let watch = tx.watch_reader();
    promises.push(watch.into_inner().await.write(vec![42]).map(Event::Write));
    promises.push(rx.read().map(Event::Read));
    let mut rx = None;
    let mut tx = None;
    while let Some(event) = promises.next(&mut store).await? {
        match event {
            Event::Write(None) => unreachable!(),
            Event::Write(Some(new_tx)) => tx = Some(new_tx),
            Event::Read(Err(_)) => unreachable!(),
            Event::Read(Ok((new_rx, values))) => {
                assert_eq!(values, vec![42]);
                rx = Some(new_rx);
            }
        }
    }
    drop(rx);
    let mut watch = tx.take().unwrap().watch_reader();
    component::get(&mut store, future::poll_fn(|cx| watch.poll_unpin(cx))).await?;
    assert!(watch
        .into_inner()
        .await
        .write(vec![42])
        .get(&mut store)
        .await?
        .is_none());

    Ok(())
}

#[tokio::test]
pub async fn async_closed_streams() -> Result<()> {
    test_closed_streams(false).await
}

#[tokio::test]
pub async fn async_closed_streams_with_watch() -> Result<()> {
    test_closed_streams(true).await
}

pub async fn test_closed_streams(watch: bool) -> Result<()> {
    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);

    let engine = Engine::new(&config)?;

    let mut store = Store::new(
        &engine,
        Ctx {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::default(),
            continue_: false,
            wakers: Arc::new(Mutex::new(None)),
        },
    );

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::add_to_linker_async(&mut linker)?;

    let component = Component::new(
        &engine,
        &fs::read(test_programs_artifacts::ASYNC_CLOSED_STREAMS_COMPONENT).await?,
    )?;

    let closed_streams =
        closed_streams::bindings::ClosedStreams::instantiate_async(&mut store, &component, &linker)
            .await?;

    #[allow(clippy::type_complexity)]
    enum StreamEvent {
        FirstWrite(Option<StreamWriter<Vec<u8>>>),
        FirstRead(Result<(StreamReader<Vec<u8>>, Vec<u8>), Option<ErrorContext>>),
        SecondWrite(Option<StreamWriter<Vec<u8>>>),
        GuestCompleted,
    }

    enum FutureEvent {
        Write(bool),
        Read(Result<u8, Option<ErrorContext>>),
        WriteIgnored(bool),
        GuestCompleted,
    }

    let values = vec![42_u8, 43, 44];

    let value = 42_u8;

    // First, test stream host->host
    {
        let (tx, rx) = component::stream(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(tx.write(values.clone()).map(StreamEvent::FirstWrite));
        promises.push(rx.read().map(StreamEvent::FirstRead));

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                StreamEvent::FirstWrite(Some(tx)) => {
                    if watch {
                        promises.push(
                            Promise::from(tx.watch_reader())
                                .map(|()| StreamEvent::SecondWrite(None)),
                        );
                    } else {
                        promises.push(tx.write(values.clone()).map(StreamEvent::SecondWrite));
                    }
                }
                StreamEvent::FirstWrite(None) => panic!("first write should have been accepted"),
                StreamEvent::FirstRead(Ok((_, results))) => {
                    assert_eq!(values, results);
                }
                StreamEvent::FirstRead(Err(_)) => unreachable!(),
                StreamEvent::SecondWrite(None) => {}
                StreamEvent::SecondWrite(Some(_)) => {
                    panic!("second write should _not_ have been accepted")
                }
                StreamEvent::GuestCompleted => unreachable!(),
            }
        }

        assert_eq!(count, 3);
    }

    // Next, test futures host->host
    {
        let (tx, rx) = component::future(&mut store)?;
        let (tx_ignored, rx_ignored) = component::future(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(tx.write(value).map(FutureEvent::Write));
        promises.push(rx.read().map(FutureEvent::Read));
        if watch {
            promises.push(
                Promise::from(tx_ignored.watch_reader()).map(|()| FutureEvent::WriteIgnored(false)),
            );
        } else {
            promises.push(tx_ignored.write(value).map(FutureEvent::WriteIgnored));
        }
        drop(rx_ignored);

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                FutureEvent::Write(delivered) => {
                    assert!(delivered);
                }
                FutureEvent::Read(Ok(result)) => {
                    assert_eq!(value, result);
                }
                FutureEvent::Read(Err(_)) => panic!("read should have succeeded"),
                FutureEvent::WriteIgnored(delivered) => {
                    assert!(!delivered);
                }
                FutureEvent::GuestCompleted => unreachable!(),
            }
        }

        assert_eq!(count, 3);
    }

    // Next, test stream host->guest
    {
        let (tx, rx) = component::stream(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(
            closed_streams
                .local_local_closed()
                .call_read_stream(&mut store, rx.into(), values.clone())
                .await?
                .map(|()| StreamEvent::GuestCompleted),
        );
        promises.push(tx.write(values.clone()).map(StreamEvent::FirstWrite));

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                StreamEvent::FirstWrite(Some(tx)) => {
                    if watch {
                        promises.push(
                            Promise::from(tx.watch_reader())
                                .map(|()| StreamEvent::SecondWrite(None)),
                        );
                    } else {
                        promises.push(tx.write(values.clone()).map(StreamEvent::SecondWrite));
                    }
                }
                StreamEvent::FirstWrite(None) => panic!("first write should have been accepted"),
                StreamEvent::FirstRead(_) => unreachable!(),
                StreamEvent::SecondWrite(None) => {}
                StreamEvent::SecondWrite(Some(_)) => {
                    panic!("second write should _not_ have been accepted")
                }
                StreamEvent::GuestCompleted => {}
            }
        }

        assert_eq!(count, 3);
    }

    // Next, test futures host->guest
    {
        let (tx, rx) = component::future(&mut store)?;
        let (tx_ignored, rx_ignored) = component::future(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(
            closed_streams
                .local_local_closed()
                .call_read_future(&mut store, rx.into(), value, rx_ignored.into())
                .await?
                .map(|()| FutureEvent::GuestCompleted),
        );
        promises.push(tx.write(value).map(FutureEvent::Write));
        if watch {
            promises.push(
                Promise::from(tx_ignored.watch_reader()).map(|()| FutureEvent::WriteIgnored(false)),
            );
        } else {
            promises.push(tx_ignored.write(value).map(FutureEvent::WriteIgnored));
        }

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                FutureEvent::Write(delivered) => {
                    assert!(delivered);
                }
                FutureEvent::Read(_) => unreachable!(),
                FutureEvent::WriteIgnored(delivered) => {
                    assert!(!delivered);
                }
                FutureEvent::GuestCompleted => {}
            }
        }

        assert_eq!(count, 3);
    }

    Ok(())
}
