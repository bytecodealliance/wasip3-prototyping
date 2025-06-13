use anyhow::Result;

use component_async_tests::util::test_run;

#[tokio::test]
pub async fn async_error_context() -> Result<()> {
    test_run(&[test_programs_artifacts::ASYNC_ERROR_CONTEXT_COMPONENT]).await
}

#[tokio::test]
pub async fn async_error_context_callee() -> Result<()> {
    test_run(&[test_programs_artifacts::ASYNC_ERROR_CONTEXT_COMPONENT]).await
}

#[tokio::test]
pub async fn async_error_context_caller() -> Result<()> {
    test_run(&[
        test_programs_artifacts::ASYNC_ERROR_CONTEXT_CALLER_COMPONENT,
        test_programs_artifacts::ASYNC_ERROR_CONTEXT_CALLEE_COMPONENT,
    ])
    .await
}
