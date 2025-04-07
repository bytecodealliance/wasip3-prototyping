use super::run;
use test_programs_artifacts::*;

foreach_p3_random!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_random_imports() -> anyhow::Result<()> {
    run(P3_RANDOM_IMPORTS_COMPONENT).await
}
