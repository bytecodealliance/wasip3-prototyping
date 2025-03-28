use anyhow::Result;
use tokio::fs;

use component_async_tests::util::{compose, test_run};

#[tokio::test]
pub async fn async_error_context() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_COMPONENT).await?).await
}

#[tokio::test]
pub async fn async_error_context_callee() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_COMPONENT).await?).await
}

#[tokio::test]
pub async fn async_error_context_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}

#[tokio::test]
async fn async_error_context_roundtrip() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}
