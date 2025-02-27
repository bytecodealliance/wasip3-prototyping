use super::run;
use test_programs_artifacts::*;

foreach_sockets_0_3!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_ip_name_lookup() -> anyhow::Result<()> {
    run(SOCKETS_0_3_IP_NAME_LOOKUP_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_bind() -> anyhow::Result<()> {
    run(SOCKETS_0_3_TCP_BIND_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_connect() -> anyhow::Result<()> {
    run(SOCKETS_0_3_TCP_CONNECT_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_sample_application() -> anyhow::Result<()> {
    run(SOCKETS_0_3_TCP_SAMPLE_APPLICATION_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_sockopts() -> anyhow::Result<()> {
    run(SOCKETS_0_3_TCP_SOCKOPTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_states() -> anyhow::Result<()> {
    run(SOCKETS_0_3_TCP_STATES_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_tcp_streams() -> anyhow::Result<()> {
    run(SOCKETS_0_3_TCP_STREAMS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_udp_bind() -> anyhow::Result<()> {
    run(SOCKETS_0_3_UDP_BIND_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_udp_connect() -> anyhow::Result<()> {
    run(SOCKETS_0_3_UDP_CONNECT_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
#[ignore = "https://github.com/bytecodealliance/wasip3-prototyping/issues/44"]
async fn sockets_0_3_udp_sample_application() -> anyhow::Result<()> {
    run(SOCKETS_0_3_UDP_SAMPLE_APPLICATION_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_udp_sockopts() -> anyhow::Result<()> {
    run(SOCKETS_0_3_UDP_SOCKOPTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn sockets_0_3_udp_states() -> anyhow::Result<()> {
    run(SOCKETS_0_3_UDP_STATES_COMPONENT).await
}
