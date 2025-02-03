use super::run;
use test_programs_artifacts::*;

foreach_clocks_0_3!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn clocks_0_3_sleep() -> anyhow::Result<()> {
    run(CLOCKS_0_3_SLEEP_COMPONENT).await
}
