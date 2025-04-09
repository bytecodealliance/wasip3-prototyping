use std::sync::{Arc, Mutex};
use std::task::Waker;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{IoView, WasiP2Ctx, WasiView};

pub mod borrowing_host;
pub mod closed_streams;
pub mod proxy;
pub mod resource_stream;
pub mod round_trip;
pub mod round_trip_direct;
pub mod round_trip_many;
pub mod transmit;
pub mod util;
pub mod yield_host;

/// Host implementation, usable primarily by tests
pub struct Ctx {
    pub wasi: WasiP2Ctx,
    pub table: ResourceTable,
    pub wakers: Arc<Mutex<Option<Vec<Waker>>>>,
    pub continue_: bool,
}

impl IoView for Ctx {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> &mut WasiP2Ctx {
        &mut self.wasi
    }
}
