use futures::{join, SinkExt as _, StreamExt as _};
use test_programs::p3::wasi::sockets::types::{
    IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress, TcpSocket,
};
use test_programs::p3::wit_stream;

struct Component;

test_programs::p3::export!(Component);

async fn test_tcp_sample_application(family: IpAddressFamily, bind_address: IpSocketAddress) {
    let first_message = b"Hello, world!";
    let second_message = b"Greetings, planet!";

    let listener = TcpSocket::new(family);

    listener.bind(bind_address).unwrap();
    listener.set_listen_backlog_size(32).unwrap();
    let mut accept = listener.listen().unwrap();

    let addr = listener.local_address().unwrap();

    {
        let client = TcpSocket::new(family);
        client.connect(addr).await.unwrap();
        let (mut data_tx, data_rx) = wit_stream::new();

        join!(
            async {
                client.send(data_rx).await.unwrap();
            },
            async {
                data_tx.send(vec![]).await.unwrap();
                data_tx.send(first_message.into()).await.unwrap();
                drop(data_tx);
            }
        );
    }

    {
        let mut sock = accept.next().await.unwrap().unwrap();
        assert_eq!(sock.len(), 1);
        let sock = sock.pop().unwrap();

        let (mut data_rx, fut) = sock.receive();
        let data = data_rx.next().await.unwrap().unwrap();

        // Check that we sent and received our message!
        assert_eq!(data, first_message); // Not guaranteed to work but should work in practice.
        fut.await.unwrap().unwrap().unwrap()
    }

    // Another client
    {
        let client = TcpSocket::new(family);
        client.connect(addr).await.unwrap();
        let (mut data_tx, data_rx) = wit_stream::new();
        join!(
            async {
                client.send(data_rx).await.unwrap();
            },
            async {
                data_tx.send(second_message.into()).await.unwrap();
                drop(data_tx);
            }
        );
    }

    {
        let mut sock = accept.next().await.unwrap().unwrap();
        assert_eq!(sock.len(), 1);
        let sock = sock.pop().unwrap();
        let (mut data_rx, fut) = sock.receive();
        let data = data_rx.next().await.unwrap().unwrap();

        // Check that we sent and received our message!
        assert_eq!(data, second_message); // Not guaranteed to work but should work in practice.
        fut.await.unwrap().unwrap().unwrap()
    }
}

impl test_programs::p3::exports::wasi::cli::run::Guest for Component {
    async fn run() -> Result<(), ()> {
        test_tcp_sample_application(
            IpAddressFamily::Ipv4,
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port: 0,                 // use any free port
                address: (127, 0, 0, 1), // localhost
            }),
        )
        .await;
        test_tcp_sample_application(
            IpAddressFamily::Ipv6,
            IpSocketAddress::Ipv6(Ipv6SocketAddress {
                port: 0,                           // use any free port
                address: (0, 0, 0, 0, 0, 0, 0, 1), // localhost
                flow_info: 0,
                scope_id: 0,
            }),
        )
        .await;
        Ok(())
    }
}

fn main() {}
