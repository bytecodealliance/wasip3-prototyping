macro_rules! assert_test_exists {
    ($name:ident) => {
        #[expect(unused_imports, reason = "just here to ensure a name exists")]
        use self::$name as _;
    };
}
test_programs_artifacts::foreach_async!(assert_test_exists);

mod backpressure;
mod borrowing;
mod error_context;
mod post_return;
mod proxy;
mod read_resource_stream;
mod round_trip;
mod round_trip_direct;
mod round_trip_many;
mod transmit;
mod unit_stream;
mod yield_;

use backpressure::{async_backpressure_callee, async_backpressure_caller};
use borrowing::{async_borrowing_callee, async_borrowing_caller};
use error_context::{
    async_error_context, async_error_context_callee, async_error_context_caller,
    async_error_context_future_callee, async_error_context_future_caller,
    async_error_context_stream_callee, async_error_context_stream_caller,
};
use post_return::{async_post_return_callee, async_post_return_caller};
use proxy::{async_http_echo, async_http_middleware};
use read_resource_stream::async_read_resource_stream;
use round_trip::{
    async_round_trip_stackful, async_round_trip_stackless, async_round_trip_synchronous,
    async_round_trip_wait,
};
use round_trip_direct::async_round_trip_direct_stackless;
use round_trip_many::{
    async_round_trip_many_stackful, async_round_trip_many_stackless,
    async_round_trip_many_synchronous, async_round_trip_many_wait,
};
use transmit::{async_poll, async_transmit_callee, async_transmit_caller};
use unit_stream::{async_unit_stream_callee, async_unit_stream_caller};
use yield_::{async_yield_callee, async_yield_caller};
