use super::run;
use test_programs_artifacts::*;

foreach_filesystem_0_3!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn filesystem_0_3_file_read_write() -> anyhow::Result<()> {
    run(FILESYSTEM_0_3_FILE_READ_WRITE_COMPONENT).await
}
