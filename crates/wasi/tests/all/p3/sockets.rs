use super::run;
use test_programs_artifacts::*;

foreach_sockets_0_3!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_ip_name_lookup() -> anyhow::Result<()> {
    run(SOCKETS_0_3_IP_NAME_LOOKUP_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_bind() -> anyhow::Result<()> {
    // TODO: uncomment
    // run(SOCKETS_0_3_TCP_BIND_COMPONENT).await
    Ok(())
}
