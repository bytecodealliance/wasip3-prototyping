use anyhow::Result;
use tokio::fs;

mod common;
use common::{compose, test_run};

#[tokio::test]
async fn async_error_context() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_COMPONENT).await?).await
}

#[tokio::test]
async fn async_error_context_callee() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_COMPONENT).await?).await
}

#[tokio::test]
async fn async_error_context_caller() -> Result<()> {
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

// No-op function; we only test this by composing it in `async_error_context_stream_callee`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
fn async_error_context_stream_callee() {}

// No-op function; we only test this by composing it in `async_error_context_stream_caller`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
fn async_error_context_stream_caller() {}

#[tokio::test]
async fn async_stream_end_err() -> Result<()> {
    let caller =
        &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_STREAM_CALLER_COMPONENT).await?;
    let callee =
        &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_STREAM_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}

// No-op function; we only test this by composing it in `async_future_end_err`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
fn async_error_context_future_callee() {}

// No-op function; we only test this by composing it in `async_future_end_err`
#[allow(
    dead_code,
    reason = "here only to make the `assert_test_exists` macro happy"
)]
fn async_error_context_future_caller() {}

#[tokio::test]
async fn async_future_end_err() -> Result<()> {
    let caller =
        &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_FUTURE_CALLER_COMPONENT).await?;
    let callee =
        &fs::read(test_programs_artifacts::ASYNC_ERROR_CONTEXT_FUTURE_CALLEE_COMPONENT).await?;
    test_run(&compose(caller, callee).await?).await
}
