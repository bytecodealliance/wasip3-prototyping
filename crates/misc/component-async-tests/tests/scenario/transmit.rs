use std::future::Future;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use tokio::fs;
use wasmtime::component::{
    Component, HostFuture, HostStream, Instance, Linker, Promise, PromisesUnordered, ResourceTable,
    Single, StreamReader, StreamWriter, Val,
};
use wasmtime::{AsContextMut, Config, Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use component_async_tests::transmit::bindings::exports::local::local::transmit::Control;
use component_async_tests::util::{compose, init_logger, test_run};
use component_async_tests::{transmit, Ctx};

#[tokio::test]
pub async fn async_poll_synchronous() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_POLL_SYNCHRONOUS_COMPONENT).await?).await
}

#[tokio::test]
pub async fn async_poll_stackless() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_POLL_STACKLESS_COMPONENT).await?).await
}

#[tokio::test]
pub async fn async_transmit_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}

#[tokio::test]
pub async fn async_transmit_callee() -> Result<()> {
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
    ) -> impl Future<Output = Result<(Self::Instance, Instance)>>;

    fn call(
        store: impl AsContextMut<Data = Ctx>,
        instance: &Self::Instance,
        params: Self::Params,
    ) -> impl Future<Output = Result<Promise<Self::Result>>>;

    fn into_params(
        control: HostStream<Control>,
        caller_stream: HostStream<String>,
        caller_future1: HostFuture<String>,
        caller_future2: HostFuture<String>,
    ) -> Self::Params;

    fn from_result(
        store: impl AsContextMut<Data = Ctx>,
        instance: Instance,
        result: Self::Result,
    ) -> Result<(HostStream<String>, HostFuture<String>, HostFuture<String>)>;
}

struct StaticTransmitTest;

impl TransmitTest for StaticTransmitTest {
    type Instance = transmit::bindings::TransmitCallee;
    type Params = (
        HostStream<Control>,
        HostStream<String>,
        HostFuture<String>,
        HostFuture<String>,
    );
    type Result = (HostStream<String>, HostFuture<String>, HostFuture<String>);

    async fn instantiate(
        mut store: impl AsContextMut<Data = Ctx>,
        component: &Component,
        linker: &Linker<Ctx>,
    ) -> Result<(Self::Instance, Instance)> {
        let instance = linker.instantiate_async(&mut store, component).await?;
        let callee = transmit::bindings::TransmitCallee::new(store, &instance)?;
        Ok((callee, instance))
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
        control: HostStream<Control>,
        caller_stream: HostStream<String>,
        caller_future1: HostFuture<String>,
        caller_future2: HostFuture<String>,
    ) -> Self::Params {
        (control, caller_stream, caller_future1, caller_future2)
    }

    fn from_result(
        _: impl AsContextMut<Data = Ctx>,
        _: Instance,
        result: Self::Result,
    ) -> Result<(HostStream<String>, HostFuture<String>, HostFuture<String>)> {
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
    ) -> Result<(Self::Instance, Instance)> {
        let instance = linker.instantiate_async(store, component).await?;
        Ok((instance, instance))
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
        control: HostStream<Control>,
        caller_stream: HostStream<String>,
        caller_future1: HostFuture<String>,
        caller_future2: HostFuture<String>,
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
        instance: Instance,
        result: Self::Result,
    ) -> Result<(HostStream<String>, HostFuture<String>, HostFuture<String>)> {
        let Val::Tuple(fields) = result else {
            unreachable!()
        };
        let stream = HostStream::from_val(store.as_context_mut(), instance, &fields[0])?;
        let future1 = HostFuture::from_val(store.as_context_mut(), instance, &fields[1])?;
        let future2 = HostFuture::from_val(store.as_context_mut(), instance, &fields[2])?;
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

    let (test, instance) = Test::instantiate(&mut store, &component, &linker).await?;

    #[allow(clippy::type_complexity)]
    enum Event<Test: TransmitTest> {
        Result(Test::Result),
        ControlWriteA(Option<StreamWriter<Single<Control>>>),
        ControlWriteB(Option<StreamWriter<Single<Control>>>),
        ControlWriteC(Option<StreamWriter<Single<Control>>>),
        ControlWriteD,
        WriteA,
        WriteB(bool),
        ReadC(Option<StreamReader<Single<String>>>, Single<String>),
        ReadD(Option<String>),
        ReadNone(Option<StreamReader<Single<String>>>),
    }

    let (control_tx, control_rx) = instance.stream::<_, _, Single<_>, _, _>(&mut store)?;
    let (caller_stream_tx, caller_stream_rx) =
        instance.stream::<_, _, Single<_>, _, _>(&mut store)?;
    let (caller_future1_tx, caller_future1_rx) = instance.future(&mut store)?;
    let (_caller_future2_tx, caller_future2_rx) = instance.future(&mut store)?;

    let mut promises = PromisesUnordered::<Event<Test>>::new();
    let mut caller_future1_tx = Some(caller_future1_tx);
    let mut callee_stream_rx = None;
    let mut callee_future1_rx = None;
    let mut complete = false;

    promises.push(
        control_tx
            .write(Single::new(Control::ReadStream("a".into())))
            .map(|(w, _)| Event::ControlWriteA(w)),
    );

    promises.push(
        caller_stream_tx
            .write(Single::new(String::from("a")))
            .map(|_| Event::WriteA),
    );

    promises.push(
        Test::call(
            &mut store,
            &test,
            Test::into_params(
                control_rx.into(),
                caller_stream_rx.into(),
                caller_future1_rx.into(),
                caller_future2_rx.into(),
            ),
        )
        .await?
        .map(Event::Result),
    );

    while let Some(event) = promises.next(&mut store).await? {
        match event {
            Event::Result(result) => {
                let results = Test::from_result(&mut store, instance, result)?;
                callee_stream_rx = Some(results.0.into_reader(&mut store));
                callee_future1_rx = Some(results.1.into_reader(&mut store));
            }
            Event::ControlWriteA(tx) => {
                promises.push(
                    tx.unwrap()
                        .write(Single::new(Control::ReadFuture("b".into())))
                        .map(|(w, _)| Event::ControlWriteB(w)),
                );
            }
            Event::WriteA => {
                promises.push(
                    caller_future1_tx
                        .take()
                        .unwrap()
                        .write("b".into())
                        .map(Event::WriteB),
                );
            }
            Event::ControlWriteB(tx) => {
                promises.push(
                    tx.unwrap()
                        .write(Single::new(Control::WriteStream("c".into())))
                        .map(|(w, _)| Event::ControlWriteC(w)),
                );
            }
            Event::WriteB(delivered) => {
                assert!(delivered);
                promises.push(
                    callee_stream_rx
                        .take()
                        .unwrap()
                        .read(Single::default())
                        .map(|(r, b)| Event::ReadC(r, b)),
                );
            }
            Event::ControlWriteC(tx) => {
                promises.push(
                    tx.unwrap()
                        .write(Single::new(Control::WriteFuture("d".into())))
                        .map(|_| Event::ControlWriteD),
                );
            }
            Event::ReadC(None, _) => unreachable!(),
            Event::ReadC(Some(rx), mut value) => {
                assert_eq!(value.take().as_deref(), Some("c"));
                promises.push(callee_future1_rx.take().unwrap().read().map(Event::ReadD));
                callee_stream_rx = Some(rx);
            }
            Event::ControlWriteD => {}
            Event::ReadD(None) => unreachable!(),
            Event::ReadD(Some(value)) => {
                assert_eq!(&value, "d");
                promises.push(
                    callee_stream_rx
                        .take()
                        .unwrap()
                        .read(Single::default())
                        .map(|(r, _)| Event::ReadNone(r)),
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
