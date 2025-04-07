use super::run;
use test_programs_artifacts::*;

foreach_p3_sockets!(assert_test_exists);

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_ip_name_lookup() -> anyhow::Result<()> {
    run(P3_SOCKETS_IP_NAME_LOOKUP_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_bind() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_BIND_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_connect() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_CONNECT_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_sample_application() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_SAMPLE_APPLICATION_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_sockopts() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_SOCKOPTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_states() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_STATES_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_tcp_streams() -> anyhow::Result<()> {
    run(P3_SOCKETS_TCP_STREAMS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_bind() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_BIND_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_connect() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_CONNECT_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
#[ignore = "https://github.com/bytecodealliance/wasip3-prototyping/issues/44"]
async fn p3_sockets_udp_sample_application() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_SAMPLE_APPLICATION_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_sockopts() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_SOCKOPTS_COMPONENT).await
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn p3_sockets_udp_states() -> anyhow::Result<()> {
    run(P3_SOCKETS_UDP_STATES_COMPONENT).await
}
