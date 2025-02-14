use anyhow::Result;
use tokio::fs;

mod common;
use common::test_run;

#[tokio::test]
async fn async_read_resource_stream() -> Result<()> {
    test_run(&fs::read(test_programs_artifacts::ASYNC_READ_RESOURCE_STREAM_COMPONENT).await?).await
}
