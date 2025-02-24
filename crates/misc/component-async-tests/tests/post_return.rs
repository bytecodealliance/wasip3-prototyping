use anyhow::Result;
use tokio::fs;

use component_async_tests::util::{compose, test_run};

// No-op function; we only test this by composing it in `async_post_return_caller`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
pub fn async_post_return_callee() {}

#[tokio::test]
pub async fn async_post_return_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_POST_RETURN_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_POST_RETURN_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}
