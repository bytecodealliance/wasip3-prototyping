use {
    anyhow::Result,
    component_async_tests::{closed_streams, util::init_logger, Ctx},
    futures::future,
    std::sync::{Arc, Mutex},
    tokio::fs,
    wasmtime::{
        component::{
            Component, Linker, PromisesUnordered, ResourceTable, Single, StreamReader,
            StreamWriter, VecBuffer,
        },
        Config, Engine, Store,
    },
    wasmtime_wasi::WasiCtxBuilder,
};

#[tokio::test]
pub async fn async_watch_streams() -> Result<()> {
    init_logger();

    let mut config = Config::new();
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

    let instance = linker.instantiate_async(&mut store, &component).await?;

    // Test watching and then dropping the read end of a stream.
    let (tx, rx) = instance.stream::<u8, Single<_>, Single<_>, _, _>(&mut store)?;
    let watch = tx.watch_reader().0;
    drop(rx);
    watch.get(&mut store).await?;

    // Test dropping and then watching the read end of a stream.
    let (tx, rx) = instance.stream::<u8, Single<_>, Single<_>, _, _>(&mut store)?;
    drop(rx);
    tx.watch_reader().0.get(&mut store).await?;

    // Test watching and then dropping the write end of a stream.
    let (tx, rx) = instance.stream::<u8, Single<_>, Single<_>, _, _>(&mut store)?;
    let watch = rx.watch_writer().0;
    drop(tx);
    watch.get(&mut store).await?;

    // Test dropping and then watching the write end of a stream.
    let (tx, rx) = instance.stream::<u8, Single<_>, Single<_>, _, _>(&mut store)?;
    drop(tx);
    rx.watch_writer().0.get(&mut store).await?;

    // Test watching and then dropping the read end of a future.
    let (tx, rx) = instance.future::<u8, _, _>(&mut store)?;
    let watch = tx.watch_reader().0;
    drop(rx);
    watch.get(&mut store).await?;

    // Test dropping and then watching the read end of a future.
    let (tx, rx) = instance.future::<u8, _, _>(&mut store)?;
    drop(rx);
    tx.watch_reader().0.get(&mut store).await?;

    // Test watching and then dropping the write end of a future.
    let (tx, rx) = instance.future::<u8, _, _>(&mut store)?;
    let watch = rx.watch_writer().0;
    drop(tx);
    watch.get(&mut store).await?;

    // Test dropping and then watching the write end of a future.
    let (tx, rx) = instance.future::<u8, _, _>(&mut store)?;
    drop(tx);
    rx.watch_writer().0.get(&mut store).await?;

    #[allow(clippy::type_complexity)]
    enum Event {
        Write(Option<StreamWriter<Single<u8>>>),
        Read(Option<StreamReader<Single<u8>>>, Single<u8>),
    }

    // Test watching, then writing to, then dropping, then writing again to the
    // read end of a stream.
    let mut promises = PromisesUnordered::new();
    let (tx, rx) = instance.stream(&mut store)?;
    let watch = tx.watch_reader().1;
    promises.push(
        watch
            .into_inner()
            .write_all(Single::new(42))
            .map(|(w, _)| Event::Write(w)),
    );
    promises.push(rx.read(Single::default()).map(|(r, b)| Event::Read(r, b)));
    let mut rx = None;
    let mut tx = None;
    while let Some(event) = promises.next(&mut store).await? {
        match event {
            Event::Write(None) => unreachable!(),
            Event::Write(Some(new_tx)) => tx = Some(new_tx),
            Event::Read(None, _) => unreachable!(),
            Event::Read(Some(new_rx), mut buffer) => {
                assert_eq!(buffer.take(), Some(42));
                rx = Some(new_rx);
            }
        }
    }
    drop(rx);
    let (promise, watch) = tx.take().unwrap().watch_reader();
    let mut future = promise.into_future();
    instance
        .get(&mut store, future::poll_fn(|cx| future.as_mut().poll(cx)))
        .await?;
    assert!(watch
        .into_inner()
        .write_all(Single::new(42))
        .get(&mut store)
        .await?
        .0
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
    init_logger();

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

    let instance = linker.instantiate_async(&mut store, &component).await?;

    let closed_streams = closed_streams::bindings::ClosedStreams::new(&mut store, &instance)?;

    #[allow(clippy::type_complexity)]
    enum StreamEvent {
        FirstWrite(Option<StreamWriter<VecBuffer<u8>>>),
        FirstRead(Option<StreamReader<Vec<u8>>>, Vec<u8>),
        SecondWrite(Option<StreamWriter<VecBuffer<u8>>>),
        GuestCompleted,
    }

    enum FutureEvent {
        Write(bool),
        Read(Option<u8>),
        WriteIgnored(bool),
        GuestCompleted,
    }

    let values = vec![42_u8, 43, 44];

    let value = 42_u8;

    // First, test stream host->host
    {
        let (tx, rx) = instance.stream(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(
            tx.write_all(values.clone().into())
                .map(|(w, _)| StreamEvent::FirstWrite(w)),
        );
        promises.push(
            rx.read(Vec::with_capacity(3))
                .map(|(r, b)| StreamEvent::FirstRead(r, b)),
        );

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                StreamEvent::FirstWrite(Some(tx)) => {
                    if watch {
                        promises.push(
                            tx.watch_reader()
                                .0
                                .map(move |()| StreamEvent::SecondWrite(None)),
                        );
                    } else {
                        promises.push(
                            tx.write_all(values.clone().into())
                                .map(|(w, _)| StreamEvent::SecondWrite(w)),
                        );
                    }
                }
                StreamEvent::FirstWrite(None) => panic!("first write should have been accepted"),
                StreamEvent::FirstRead(Some(_), results) => {
                    assert_eq!(values, results);
                }
                StreamEvent::FirstRead(None, _) => unreachable!(),
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
        let (tx, rx) = instance.future(&mut store)?;
        let (tx_ignored, rx_ignored) = instance.future(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(tx.write(value).map(FutureEvent::Write));
        promises.push(rx.read().map(FutureEvent::Read));
        if watch {
            promises.push(
                tx_ignored
                    .watch_reader()
                    .0
                    .map(|()| FutureEvent::WriteIgnored(false)),
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
                FutureEvent::Read(Some(result)) => {
                    assert_eq!(value, result);
                }
                FutureEvent::Read(None) => panic!("read should have succeeded"),
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
        let (tx, rx) = instance.stream::<_, _, Vec<_>, _, _>(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(
            closed_streams
                .local_local_closed()
                .call_read_stream(&mut store, rx.into(), values.clone())
                .await?
                .map(|()| StreamEvent::GuestCompleted),
        );
        promises.push(
            tx.write_all(values.clone().into())
                .map(|(w, _)| StreamEvent::FirstWrite(w)),
        );

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                StreamEvent::FirstWrite(Some(tx)) => {
                    if watch {
                        promises.push(tx.watch_reader().0.map(|()| StreamEvent::SecondWrite(None)));
                    } else {
                        promises.push(
                            tx.write_all(values.clone().into())
                                .map(|(w, _)| StreamEvent::SecondWrite(w)),
                        );
                    }
                }
                StreamEvent::FirstWrite(None) => panic!("first write should have been accepted"),
                StreamEvent::FirstRead(_, _) => unreachable!(),
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
        let (tx, rx) = instance.future(&mut store)?;
        let (tx_ignored, rx_ignored) = instance.future(&mut store)?;

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
                tx_ignored
                    .watch_reader()
                    .0
                    .map(|()| FutureEvent::WriteIgnored(false)),
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
