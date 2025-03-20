use anyhow::Result;
use tokio::fs;

use component_async_tests::util::{compose, test_run};

// No-op function; we only test this by composing it in
// `async_yield_callee_synchronous` and `async_yield_callee_stackful`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
pub fn async_yield_caller() {}

#[tokio::test]
pub async fn async_yield_callee_synchronous() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_YIELD_CALLER_COMPONENT).await?;
    let callee =
        &fs::read(test_programs_artifacts::ASYNC_YIELD_CALLEE_SYNCHRONOUS_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}

#[tokio::test]
pub async fn async_yield_callee_stackless() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_YIELD_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_YIELD_CALLEE_STACKLESS_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}
