use anyhow::Result;
use tokio::fs;

mod common;
use common::{compose, test_run_bool};

#[tokio::test]
async fn borrowing_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    test_run_bool(&compose(caller, callee).await?, false).await
}

#[tokio::test]
async fn borrowing_caller_misbehave() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    let error = format!(
        "{:?}",
        test_run_bool(&compose(caller, callee).await?, true)
            .await
            .unwrap_err()
    );
    assert!(error.contains("unknown handle index"), "{error}");
    Ok(())
}

#[tokio::test]
async fn borrowing_callee() -> Result<()> {
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    test_run_bool(callee, false).await
}

#[tokio::test]
async fn borrowing_callee_misbehave() -> Result<()> {
    let callee = &fs::read(test_programs_artifacts::ASYNC_BORROWING_CALLEE_COMPONENT).await?;
    let error = format!("{:?}", test_run_bool(callee, true).await.unwrap_err());
    assert!(error.contains("unknown handle index"), "{error}");
    Ok(())
}
