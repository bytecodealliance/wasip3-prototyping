use std::ops::DerefMut;
use std::task::Poll;

use futures::future;
use wasmtime::component::Accessor;

use super::Ctx;

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "yield-host",
        concurrent_imports: true,
        concurrent_exports: true,
        async: {
            only_imports: [
                "local:local/ready#when-ready",
            ]
        },
    });
}

impl bindings::local::local::continue_::Host for &mut Ctx {
    fn set_continue(&mut self, v: bool) {
        self.continue_ = v;
    }

    fn get_continue(&mut self) -> bool {
        self.continue_
    }
}

impl bindings::local::local::ready::Host for &mut Ctx {
    fn set_ready(&mut self, ready: bool) {
        let mut wakers = self.wakers.lock().unwrap();
        if ready {
            if let Some(wakers) = wakers.take() {
                for waker in wakers {
                    waker.wake();
                }
            }
        } else if wakers.is_none() {
            *wakers = Some(Vec::new());
        }
    }

    async fn when_ready<T>(accessor: &mut Accessor<T, Self>) {
        let wakers = accessor.with(|view| view.wakers.clone());
        future::poll_fn(move |cx| {
            let mut wakers = wakers.lock().unwrap();
            if let Some(wakers) = wakers.deref_mut() {
                wakers.push(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await
    }
}
