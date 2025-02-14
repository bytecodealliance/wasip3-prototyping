use anyhow::Result;
use wasmtime::component::{Accessor, BackgroundTask, Resource, StreamReader, StreamWriter};
use wasmtime::AsContextMut;
use wasmtime_wasi::IoView;

use super::Ctx;

pub mod bindings {
    wasmtime::component::bindgen!({
        trappable_imports: true,
        path: "wit",
        world: "read-resource-stream",
        concurrent_imports: true,
        concurrent_exports: true,
        async: true,
        with: {
            "local:local/resource-stream/x": super::ResourceStreamX,
        }
    });
}

pub struct ResourceStreamX;

impl bindings::local::local::resource_stream::HostX for &mut Ctx {
    async fn foo<T>(accessor: &mut Accessor<T, Self>, x: Resource<ResourceStreamX>) -> Result<()> {
        accessor.with(|mut view| {
            _ = IoView::table(&mut *view).get(&x)?;
            Ok(())
        })
    }

    async fn drop(&mut self, x: Resource<ResourceStreamX>) -> Result<()> {
        IoView::table(self).delete(x)?;
        Ok(())
    }
}

impl bindings::local::local::resource_stream::Host for &mut Ctx {
    async fn foo<T: 'static>(
        accessor: &mut Accessor<T, Self>,
        count: u32,
    ) -> wasmtime::Result<StreamReader<Resource<ResourceStreamX>>> {
        struct Task {
            tx: StreamWriter<Resource<ResourceStreamX>>,
            count: u32,
        }

        impl<T, U: wasmtime_wasi::IoView> BackgroundTask<T, U> for Task {
            async fn run(self, accessor: &mut Accessor<T, U>) -> Result<()> {
                let mut tx = self.tx;
                for _ in 0..self.count {
                    tx = accessor
                        .with(|mut view| {
                            let item = IoView::table(&mut *view).push(ResourceStreamX)?;
                            Ok::<_, anyhow::Error>(
                                tx.write(view.as_context_mut(), vec![item])?.into_future(),
                            )
                        })?
                        .await;
                }
                accessor.with(|mut view| tx.close(view.as_context_mut()))
            }
        }

        let (tx, rx) =
            accessor.with(|mut view| wasmtime::component::stream(view.as_context_mut()))?;
        accessor.spawn(Task { tx, count });
        Ok(rx)
    }
}
