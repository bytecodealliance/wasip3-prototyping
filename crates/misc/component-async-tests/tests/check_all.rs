// This block of imports re-exports every named test to ensure that we always have coverage
// for all test modules (see use of the assert_test_exists macro)

// macro_rules! assert_test_exists {
//     ($name:ident) => {
//         #[expect(unused_imports, reason = "just here to ensure a name exists")]
//         use self::$name as _;
//     };
// }
// test_programs_artifacts::foreach_async!(assert_test_exists);

// mod backpressure;
// mod borrowing;
// mod error_context;
// mod post_return;
// mod proxy;
// mod read_resource_stream;
// mod round_trip;
// mod round_trip_many;
// mod transmit;
// mod unit_stream;
// mod yield_;

// pub use borrowing::async_borrowing_caller;
// pub use transmit::{async_transmit_callee, async_transmit_caller};
