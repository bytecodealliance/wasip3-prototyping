use anyhow::Result;
use tokio::fs;

mod common;
use common::{compose, test_run, test_transmit};

#[tokio::test]
async fn async_poll() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_POLL_COMPONENT).await?).await
}

#[tokio::test]
async fn async_transmit_caller() -> Result<()> {
    let caller = &fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLER_COMPONENT).await?;
    let callee = &fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}

#[tokio::test]
async fn async_transmit_callee() -> Result<()> {
    test_transmit(&fs::read(test_programs_artifacts::ASYNC_TRANSMIT_CALLEE_COMPONENT).await?).await
}
