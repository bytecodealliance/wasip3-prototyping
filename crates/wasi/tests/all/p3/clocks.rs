use super::run;
use test_programs_artifacts::*;

foreach_p3_clocks!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_clocks_sleep() -> anyhow::Result<()> {
    run(P3_CLOCKS_SLEEP_COMPONENT).await
}
