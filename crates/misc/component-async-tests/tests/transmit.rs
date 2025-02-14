use std::future::Future;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use tokio::fs;
use wasmtime::component::{
    self, Component, ErrorContext, FutureReader, Instance, Linker, Promise, PromisesUnordered,
    ResourceTable, StreamReader, StreamWriter, Val,
};
use wasmtime::{AsContextMut, Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::transmit::bindings::exports::local::local::transmit::Control;
use component_async_tests::{transmit, Ctx};

mod common;
use common::{compose, init_logger, test_run};

#[tokio::test]
async fn async_poll() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_POLL_COMPONENT).await?).await
}

#[tokio::test]
async fn async_transmit_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}

#[tokio::test]
async fn async_transmit_callee() -> Result<()> {
    test_transmit(&fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLEE_COMPONENT).await?).await
}

pub trait TransmitTest {
    type Instance;
    type Params;
    type Result: Send + Sync + 'static;

    fn instantiate(
        store: impl AsContextMut<Data = Ctx>,
        component: &Component,
        linker: &Linker<Ctx>,
    ) -> impl Future<Output = Result<Self::Instance>>;

    fn call(
        store: impl AsContextMut<Data = Ctx>,
        instance: &Self::Instance,
        params: Self::Params,
    ) -> impl Future<Output = Result<Promise<Self::Result>>>;

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
    type Instance = transmit::bindings::TransmitCallee;
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
        transmit::bindings::TransmitCallee::instantiate_async(store, component, linker).await
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

async fn test_transmit(component: &[u8]) -> Result<()> {
    init_logger();

    test_transmit_with::<StaticTransmitTest>(component).await?;
    test_transmit_with::<DynamicTransmitTest>(component).await
}

async fn test_transmit_with<Test: TransmitTest + 'static>(component: &[u8]) -> Result<()> {
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

    let (control_tx, control_rx) = component::stream(&mut store)?;
    let (caller_stream_tx, caller_stream_rx) = component::stream(&mut store)?;
    let (caller_future1_tx, caller_future1_rx) = component::future(&mut store)?;
    let (_caller_future2_tx, caller_future2_rx) = component::future(&mut store)?;

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
