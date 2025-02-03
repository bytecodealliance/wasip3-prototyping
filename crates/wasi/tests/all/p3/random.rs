use super::run;
use test_programs_artifacts::*;

foreach_random_0_3!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn random_0_3_imports() -> anyhow::Result<()> {
    run(RANDOM_0_3_IMPORTS_COMPONENT).await
}
