use {
    anyhow::Result,
    component_async_tests::{closed_streams, Ctx},
    std::sync::{Arc, Mutex},
    tokio::fs,
    wasmtime::{
        component::{
            self, Component, ErrorContext, Linker, PromisesUnordered, ResourceTable, StreamReader,
            StreamWriter,
        },
        Config, Engine, Store,
    },
    wasmtime_wasi::WasiCtxBuilder,
};

#[tokio::test]
pub async fn async_closed_streams() -> Result<()> {
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

    enum StreamEvent {
        FirstWrite(Option<StreamWriter<u8>>),
        FirstRead(Option<(StreamReader<u8>, Vec<u8>)>),
        SecondWrite(Option<StreamWriter<u8>>),
        GuestCompleted,
    }

    enum FutureEvent {
        Write(bool),
        Read(Option<Result<u8, ErrorContext>>),
        WriteIgnored(bool),
        GuestCompleted,
    }

    let values = vec![42_u8, 43, 44];

    let value = 42_u8;

    // First, test stream host->host
    {
        let (tx, rx) = component::stream(&mut store)?;

        let mut promises = PromisesUnordered::new();
        promises.push(
            tx.write(&mut store, values.clone())?
                .map(StreamEvent::FirstWrite),
        );
        promises.push(rx.read(&mut store)?.map(StreamEvent::FirstRead));

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                StreamEvent::FirstWrite(Some(tx)) => {
                    promises.push(
                        tx.write(&mut store, values.clone())?
                            .map(StreamEvent::SecondWrite),
                    );
                }
                StreamEvent::FirstWrite(None) => panic!("first write should have been accepted"),
                StreamEvent::FirstRead(Some((rx, results))) => {
                    assert_eq!(values, results);
                    rx.close(&mut store)?;
                }
                StreamEvent::FirstRead(None) => unreachable!(),
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
        promises.push(tx.write(&mut store, value)?.map(FutureEvent::Write));
        promises.push(rx.read(&mut store)?.map(FutureEvent::Read));
        promises.push(
            tx_ignored
                .write(&mut store, value)?
                .map(FutureEvent::WriteIgnored),
        );
        rx_ignored.close(&mut store)?;

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                FutureEvent::Write(delivered) => {
                    assert!(delivered);
                }
                FutureEvent::Read(Some(Ok(result))) => {
                    assert_eq!(value, result);
                }
                FutureEvent::Read(_) => panic!("read should have succeeded"),
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
                .call_read_stream(&mut store, rx, values.clone())
                .await?
                .map(|()| StreamEvent::GuestCompleted),
        );
        promises.push(
            tx.write(&mut store, values.clone())?
                .map(StreamEvent::FirstWrite),
        );

        let mut count = 0;
        while let Some(event) = promises.next(&mut store).await? {
            count += 1;
            match event {
                StreamEvent::FirstWrite(Some(tx)) => {
                    promises.push(
                        tx.write(&mut store, values.clone())?
                            .map(StreamEvent::SecondWrite),
                    );
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
                .call_read_future(&mut store, rx, value, rx_ignored)
                .await?
                .map(|()| FutureEvent::GuestCompleted),
        );
        promises.push(tx.write(&mut store, value)?.map(FutureEvent::Write));
        promises.push(
            tx_ignored
                .write(&mut store, value)?
                .map(FutureEvent::WriteIgnored),
        );

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
