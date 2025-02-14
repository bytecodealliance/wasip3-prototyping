// macro_rules! assert_test_exists {
//     ($name:ident) => {
//         #[expect(unused_imports, reason = "just here to ensure a name exists")]
//         use self::$name as _;
//     };
// }

// test_programs_artifacts::foreach_async!(assert_test_exists);

use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::fs;
use wasm_compose::composer::ComponentComposer;
use wasmtime::component::{
    Component, ErrorContext, FutureReader, Instance, Linker, Promise,
    PromisesUnordered, ResourceTable, StreamReader, StreamWriter, Val,
};
use wasmtime::{AsContextMut, Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::transmit::bindings::exports::local::local::transmit::Control;
use component_async_tests::Ctx;

pub fn init_logger() {
    static ONCE: Once = Once::new();
    ONCE.call_once(pretty_env_logger::init);
}

/// Compose two components
///
/// a is the "root" component, and b is composed into it
pub async fn compose(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir()?;

    let a_file = dir.path().join("a.wasm");
    fs::write(&a_file, a).await?;

    let b_file = dir.path().join("b.wasm");
    fs::write(&b_file, b).await?;

    ComponentComposer::new(
        &a_file,
        &wasm_compose::config::Config {
            dir: dir.path().to_owned(),
            definitions: vec![b_file.to_owned()],
            ..Default::default()
        },
    )
    .compose()
}

pub async fn test_run(component: &[u8]) -> Result<()> {
    init_logger();

    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);
    config.epoch_interruption(true);

    let engine = Engine::new(&config)?;

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    component_async_tests::yield_host::bindings::YieldHost::add_to_linker(&mut linker, |ctx| ctx)?;

    let mut store = Store::new(
        &engine,
        Ctx {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::default(),
            continue_: false,
            wakers: Arc::new(Mutex::new(None)),
        },
    );
    store.set_epoch_deadline(1);

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        engine.increment_epoch();
    });

    let yield_host = component_async_tests::yield_host::bindings::YieldHost::instantiate_async(
        &mut store, &component, &linker,
    )
    .await?;

    // Start three concurrent calls and then join them all:
    let mut promises = PromisesUnordered::new();
    for _ in 0..3 {
        promises.push(yield_host.local_local_run().call_run(&mut store).await?);
    }

    while let Some(()) = promises.next(&mut store).await? {
        // continue
    }

    Ok(())
}

pub async fn test_run_bool(component: &[u8], v: bool) -> Result<()> {
    init_logger();

    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);
    config.epoch_interruption(true);

    let engine = Engine::new(&config)?;

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    component_async_tests::borrowing_host::bindings::BorrowingHost::add_to_linker(
        &mut linker,
        |ctx| ctx,
    )?;

    let mut store = Store::new(
        &engine,
        Ctx {
            wasi: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::default(),
            continue_: false,
            wakers: Arc::new(Mutex::new(None)),
        },
    );
    store.set_epoch_deadline(1);

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        engine.increment_epoch();
    });

    let borrowing_host =
        component_async_tests::borrowing_host::bindings::BorrowingHost::instantiate_async(
            &mut store, &component, &linker,
        )
        .await?;

    // Start three concurrent calls and then join them all:
    let mut promises = PromisesUnordered::new();
    for _ in 0..3 {
        promises.push(
            borrowing_host
                .local_local_run_bool()
                .call_run(&mut store, v)
                .await?,
        );
    }

    while let Some(()) = promises.next(&mut store).await? {
        // continue
    }

    Ok(())
}

pub trait TransmitTest {
    type Instance;
    type Params;
    type Result;

    async fn instantiate(
        store: impl AsContextMut<Data = Ctx>,
        component: &Component,
        linker: &Linker<Ctx>,
    ) -> Result<Self::Instance>;

    async fn call(
        store: impl AsContextMut<Data = Ctx>,
        instance: &Self::Instance,
        params: Self::Params,
    ) -> Result<Promise<Self::Result>>;

    fn into_params(
        control: StreamReader<Control>,
        caller_stream: StreamReader<String>,
        caller_future1: FutureReader<String>,
        caller_future2: FutureReader<String>,
    ) -> Self::Params;

    fn from_result(
        store: impl AsContextMut<Data = Ctx>,
        result: Self::Result,
    ) -> Result<(
        StreamReader<String>,
        FutureReader<String>,
        FutureReader<String>,
    )>;
}

struct StaticTransmitTest;

impl TransmitTest for StaticTransmitTest {
    type Instance = component_async_tests::transmit::bindings::TransmitCallee;
    type Params = (
        StreamReader<Control>,
        StreamReader<String>,
        FutureReader<String>,
        FutureReader<String>,
    );
    type Result = (
        StreamReader<String>,
        FutureReader<String>,
        FutureReader<String>,
    );

    async fn instantiate(
        store: impl AsContextMut<Data = Ctx>,
        component: &Component,
        linker: &Linker<Ctx>,
    ) -> Result<Self::Instance> {
        component_async_tests::transmit::bindings::TransmitCallee::instantiate_async(
            store, component, linker,
        )
        .await
    }

    async fn call(
        store: impl AsContextMut<Data = Ctx>,
        instance: &Self::Instance,
        params: Self::Params,
    ) -> Result<Promise<Self::Result>> {
        instance
            .local_local_transmit()
            .call_exchange(store, params.0, params.1, params.2, params.3)
            .await
    }

    fn into_params(
        control: StreamReader<Control>,
        caller_stream: StreamReader<String>,
        caller_future1: FutureReader<String>,
        caller_future2: FutureReader<String>,
    ) -> Self::Params {
        (control, caller_stream, caller_future1, caller_future2)
    }

    fn from_result(
        _: impl AsContextMut<Data = Ctx>,
        result: Self::Result,
    ) -> Result<(
        StreamReader<String>,
        FutureReader<String>,
        FutureReader<String>,
    )> {
        Ok(result)
    }
}

struct DynamicTransmitTest;

impl TransmitTest for DynamicTransmitTest {
    type Instance = Instance;
    type Params = Vec<Val>;
    type Result = Val;

    async fn instantiate(
        store: impl AsContextMut<Data = Ctx>,
        component: &Component,
        linker: &Linker<Ctx>,
    ) -> Result<Self::Instance> {
        linker.instantiate_async(store, component).await
    }

    async fn call(
        mut store: impl AsContextMut<Data = Ctx>,
        instance: &Self::Instance,
        params: Self::Params,
    ) -> Result<Promise<Self::Result>> {
        let transmit_instance = instance
            .get_export(store.as_context_mut(), None, "local:local/transmit")
            .ok_or_else(|| anyhow!("can't find `local:local/transmit` in instance"))?;
        let exchange_function = instance
            .get_export(store.as_context_mut(), Some(&transmit_instance), "exchange")
            .ok_or_else(|| anyhow!("can't find `exchange` in instance"))?;
        let exchange_function = instance
            .get_func(store.as_context_mut(), exchange_function)
            .ok_or_else(|| anyhow!("can't find `exchange` in instance"))?;

        Ok(exchange_function
            .call_concurrent(store, params)
            .await?
            .map(|results| results.into_iter().next().unwrap()))
    }

    fn into_params(
        control: StreamReader<Control>,
        caller_stream: StreamReader<String>,
        caller_future1: FutureReader<String>,
        caller_future2: FutureReader<String>,
    ) -> Self::Params {
        vec![
            control.into_val(),
            caller_stream.into_val(),
            caller_future1.into_val(),
            caller_future2.into_val(),
        ]
    }

    fn from_result(
        mut store: impl AsContextMut<Data = Ctx>,
        result: Self::Result,
    ) -> Result<(
        StreamReader<String>,
        FutureReader<String>,
        FutureReader<String>,
    )> {
        let Val::Tuple(fields) = result else {
            unreachable!()
        };
        let stream = StreamReader::from_val(store.as_context_mut(), &fields[0])?;
        let future1 = FutureReader::from_val(store.as_context_mut(), &fields[1])?;
        let future2 = FutureReader::from_val(store.as_context_mut(), &fields[2])?;
        Ok((stream, future1, future2))
    }
}

pub async fn test_transmit(component: &[u8]) -> Result<()> {
    init_logger();

    test_transmit_with::<StaticTransmitTest>(component).await?;
    test_transmit_with::<DynamicTransmitTest>(component).await
}

pub async fn test_transmit_with<Test: TransmitTest + 'static>(component: &[u8]) -> Result<()> {
    let mut config = Config::new();
    config.debug_info(true);
    config.cranelift_debug_verifier(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.async_support(true);

    let engine = Engine::new(&config)?;

    let make_store = || {
        Store::new(
            &engine,
            Ctx {
                wasi: WasiCtxBuilder::new().inherit_stdio().build(),
                table: ResourceTable::default(),
                continue_: false,
                wakers: Arc::new(Mutex::new(None)),
            },
        )
    };

    let component = Component::new(&engine, component)?;

    let mut linker = Linker::new(&engine);

    wasmtime_wasi::add_to_linker_async(&mut linker)?;

    let mut store = make_store();

    let instance = Test::instantiate(&mut store, &component, &linker).await?;

    enum Event<Test: TransmitTest> {
        Result(Test::Result),
        ControlWriteA(StreamWriter<Control>),
        ControlWriteB(StreamWriter<Control>),
        ControlWriteC(StreamWriter<Control>),
        ControlWriteD(StreamWriter<Control>),
        WriteA(StreamWriter<String>),
        WriteB,
        ReadC(Option<(StreamReader<String>, Vec<String>)>),
        ReadD(Option<Result<String, ErrorContext>>),
        ReadNone(Option<(StreamReader<String>, Vec<String>)>),
    }

    let (control_tx, control_rx) = wasmtime::component::stream(&mut store)?;
    let (caller_stream_tx, caller_stream_rx) = wasmtime::component::stream(&mut store)?;
    let (caller_future1_tx, caller_future1_rx) = wasmtime::component::future(&mut store)?;
    let (_caller_future2_tx, caller_future2_rx) = wasmtime::component::future(&mut store)?;

    let mut promises = PromisesUnordered::<Event<Test>>::new();
    let mut caller_future1_tx = Some(caller_future1_tx);
    let mut callee_stream_rx = None;
    let mut callee_future1_rx = None;
    let mut complete = false;

    promises.push(
        control_tx
            .write(&mut store, vec![Control::ReadStream("a".into())])?
            .map(Event::ControlWriteA),
    );

    promises.push(
        caller_stream_tx
            .write(&mut store, vec!["a".into()])?
            .map(Event::WriteA),
    );

    promises.push(
        Test::call(
            &mut store,
            &instance,
            Test::into_params(
                control_rx,
                caller_stream_rx,
                caller_future1_rx,
                caller_future2_rx,
            ),
        )
        .await?
        .map(Event::Result),
    );

    while let Some(event) = promises.next(&mut store).await? {
        match event {
            Event::Result(result) => {
                let results = Test::from_result(&mut store, result)?;
                callee_stream_rx = Some(results.0);
                callee_future1_rx = Some(results.1);
                results.2.close(&mut store)?;
            }
            Event::ControlWriteA(tx) => {
                promises.push(
                    tx.write(&mut store, vec![Control::ReadFuture("b".into())])?
                        .map(Event::ControlWriteB),
                );
            }
            Event::WriteA(tx) => {
                tx.close(&mut store)?;
                promises.push(
                    caller_future1_tx
                        .take()
                        .unwrap()
                        .write(&mut store, "b".into())?
                        .map(|()| Event::WriteB),
                );
            }
            Event::ControlWriteB(tx) => {
                promises.push(
                    tx.write(&mut store, vec![Control::WriteStream("c".into())])?
                        .map(Event::ControlWriteC),
                );
            }
            Event::WriteB => {
                promises.push(
                    callee_stream_rx
                        .take()
                        .unwrap()
                        .read(&mut store)?
                        .map(Event::ReadC),
                );
            }
            Event::ControlWriteC(tx) => {
                promises.push(
                    tx.write(&mut store, vec![Control::WriteFuture("d".into())])?
                        .map(Event::ControlWriteD),
                );
            }
            Event::ReadC(None) => unreachable!(),
            Event::ReadC(Some((rx, values))) => {
                assert_eq!("c", &values[0]);
                promises.push(
                    callee_future1_rx
                        .take()
                        .unwrap()
                        .read(&mut store)?
                        .map(Event::ReadD),
                );
                callee_stream_rx = Some(rx);
            }
            Event::ControlWriteD(tx) => {
                tx.close(&mut store)?;
            }
            Event::ReadD(None) => unreachable!(),
            Event::ReadD(Some(value)) => {
                assert!(value.is_ok_and(|v| v == "d"));
                promises.push(
                    callee_stream_rx
                        .take()
                        .unwrap()
                        .read(&mut store)?
                        .map(Event::ReadNone),
                );
            }
            Event::ReadNone(Some(_)) => unreachable!(),
            Event::ReadNone(None) => {
                complete = true;
            }
        }
    }

    assert!(complete);

    Ok(())
}
