use std::time::Duration;

use super::Ctx;
use wasmtime::component::Accessor;

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

impl local::local::sleep::Host for &mut Ctx {
    async fn sleep_millis<T>(_: &mut Accessor<T, Self>, time_in_millis: u64) {
        tokio::time::sleep(Duration::from_millis(time_in_millis)).await;
    }
}
