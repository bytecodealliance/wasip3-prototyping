use anyhow::Result;
use tokio::fs;

use component_async_tests::util::{compose, test_run_with_count};

// No-op function; we only test this by composing it in `async_unit_stream_caller`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
pub fn async_unit_stream_callee() {}

#[tokio::test]
pub async fn async_unit_stream_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_UNIT_STREAM_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_UNIT_STREAM_CALLEE_COMPONENT).await?;
    test_run_with_count(&compose(caller, callee).await?, 1).await
}
