use super::run;
use test_programs_artifacts::*;

foreach_p3_filesystem!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_filesystem_file_read_write() -> anyhow::Result<()> {
    run(P3_FILESYSTEM_FILE_READ_WRITE_COMPONENT).await
}
