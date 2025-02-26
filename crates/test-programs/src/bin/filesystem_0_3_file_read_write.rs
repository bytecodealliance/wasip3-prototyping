use futures::{join, SinkExt as _, TryStreamExt as _};
use test_programs::p3::wasi::filesystem::types::{DescriptorFlags, OpenFlags, PathFlags};
use test_programs::p3::{wasi, wit_stream};

struct Component;

test_programs::p3::export!(Component);

impl test_programs::p3::exports::wasi::cli::run::Guest for Component {
    async fn run() -> Result<(), ()> {
        let preopens = wasi::filesystem::preopens::get_directories();
        let (dir, _) = &preopens[0];

        let filename = "test.txt";
        let file = dir
            .open_at(
                PathFlags::empty(),
                filename,
                OpenFlags::CREATE,
                DescriptorFlags::READ | DescriptorFlags::WRITE,
            )
            .unwrap();
        let (mut data_tx, data_rx) = wit_stream::new();
        join!(
            async {
                file.write_via_stream(data_rx, 5).await.unwrap();
            },
            async {
                data_tx.send(b"Hello, ".to_vec()).await.unwrap();
                data_tx.send(b"World!".to_vec()).await.unwrap();
                drop(data_tx);
            },
        );
        let (data_rx, data_fut) = file.read_via_stream(0);
        let contents = data_rx.try_collect::<Vec<_>>().await.unwrap().concat();
        data_fut.await.unwrap().unwrap().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&contents),
            "\0\0\0\0\0Hello, World!"
        );

        // Test that file read streams behave like other read streams.
        let (data_rx, data_fut) = file.read_via_stream(5);
        let contents = data_rx.try_collect::<Vec<_>>().await.unwrap().concat();
        data_fut.await.unwrap().unwrap().unwrap();
        assert_eq!(String::from_utf8_lossy(&contents), "Hello, World!");

        dir.unlink_file_at(filename).unwrap();
        Ok(())
    }
}

fn main() {}
