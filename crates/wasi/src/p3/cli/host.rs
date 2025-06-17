use crate::p3::ResourceView as _;
use crate::p3::bindings::cli::{
    environment, exit, stderr, stdin, stdout, terminal_input, terminal_output, terminal_stderr,
    terminal_stdin, terminal_stdout,
};
use crate::p3::cli::{I32Exit, TerminalInput, TerminalOutput, WasiCli, WasiCliImpl, WasiCliView};
use anyhow::{Context as _, anyhow};
use bytes::BytesMut;
use std::io::Cursor;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use wasmtime::component::{
    Accessor, AccessorTask, HostStream, Resource, StreamReader, StreamWriter,
};

struct InputTask<T> {
    input: T,
    tx: StreamWriter<Cursor<BytesMut>>,
}

impl<T, U, V> AccessorTask<T, WasiCli<U>, wasmtime::Result<()>> for InputTask<V>
where
    T: 'static,
    U: 'static,
    V: AsyncRead + Send + Sync + Unpin + 'static,
{
    async fn run(mut self, _: &mut Accessor<T, WasiCli<U>>) -> wasmtime::Result<()> {
        let mut tx = self.tx;
        let mut buf = BytesMut::with_capacity(8096);
        loop {
            match self.input.read_buf(&mut buf).await {
                Ok(0) => return Ok(()),
                Ok(_) => {
                    let (Some(tail), buf_again) = tx.write_all(Cursor::new(buf)).await else {
                        break Ok(());
                    };
                    tx = tail;
                    buf = buf_again.into_inner();
                    buf.clear();
                }
                Err(_err) => {
                    // TODO: Close the stream with an error context
                    drop(tx);
                    return Ok(());
                }
            }
        }
    }
}

struct OutputTask<T> {
    output: T,
    data: StreamReader<BytesMut>,
}

impl<T, U, V> AccessorTask<T, WasiCli<U>, wasmtime::Result<()>> for OutputTask<V>
where
    T: 'static,
    U: 'static,
    V: AsyncWrite + Send + Sync + Unpin + 'static,
{
    async fn run(mut self, _: &mut Accessor<T, WasiCli<U>>) -> wasmtime::Result<()> {
        let mut buf = BytesMut::with_capacity(8096);
        let mut fut = self.data.read(buf);
        loop {
            let (Some(tail), buf_again) = fut.await else {
                return Ok(());
            };

            buf = buf_again;
            match self.output.write_all(&buf).await {
                Ok(()) => {
                    buf.clear();
                    fut = tail.read(buf);
                    continue;
                }
                Err(_err) => {
                    // TODO: Report the error to the guest
                    drop(tail);
                    return Ok(());
                }
            }
        }
    }
}

impl<T> terminal_input::Host for WasiCliImpl<T> where T: WasiCliView {}
impl<T> terminal_output::Host for WasiCliImpl<T> where T: WasiCliView {}

impl<T> terminal_input::HostTerminalInput for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn drop(&mut self, rep: Resource<TerminalInput>) -> wasmtime::Result<()> {
        self.table()
            .delete(rep)
            .context("failed to delete input resource from table")?;
        Ok(())
    }
}

impl<T> terminal_output::HostTerminalOutput for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn drop(&mut self, rep: Resource<TerminalOutput>) -> wasmtime::Result<()> {
        self.table()
            .delete(rep)
            .context("failed to delete output resource from table")?;
        Ok(())
    }
}

impl<T> terminal_stdin::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_terminal_stdin(&mut self) -> wasmtime::Result<Option<Resource<TerminalInput>>> {
        if self.cli().stdin.is_terminal() {
            let fd = self
                .table()
                .push(TerminalInput)
                .context("failed to push terminal resource to table")?;
            Ok(Some(fd))
        } else {
            Ok(None)
        }
    }
}

impl<T> terminal_stdout::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_terminal_stdout(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        if self.cli().stdout.is_terminal() {
            let fd = self
                .table()
                .push(TerminalOutput)
                .context("failed to push terminal resource to table")?;
            Ok(Some(fd))
        } else {
            Ok(None)
        }
    }
}

impl<T> terminal_stderr::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_terminal_stderr(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        if self.cli().stderr.is_terminal() {
            let fd = self
                .table()
                .push(TerminalOutput)
                .context("failed to push terminal resource to table")?;
            Ok(Some(fd))
        } else {
            Ok(None)
        }
    }
}

impl<T> stdin::HostConcurrent for WasiCli<T>
where
    T: WasiCliView + 'static,
{
    async fn get_stdin<U: 'static>(
        store: &mut Accessor<U, Self>,
    ) -> wasmtime::Result<HostStream<u8>> {
        store.with(|mut view| {
            let instance = view.instance();
            let (tx, rx) = instance
                .stream::<_, _, Vec<_>>(&mut view)
                .context("failed to create stream")?;
            let stdin = view.get().cli().stdin.reader();
            view.spawn(InputTask { input: stdin, tx });
            Ok(rx.into())
        })
    }
}

impl<T> stdin::Host for WasiCliImpl<T> where T: WasiCliView {}

impl<T> stdout::HostConcurrent for WasiCli<T>
where
    T: WasiCliView + 'static,
{
    async fn set_stdout<U: 'static>(
        store: &mut Accessor<U, Self>,
        data: HostStream<u8>,
    ) -> wasmtime::Result<()> {
        store.with(|mut view| {
            let stdout = view.get().cli().stdout.writer();
            let data = data.into_reader(&mut view);
            view.spawn(OutputTask {
                output: stdout,
                data,
            });
            Ok(())
        })
    }
}

impl<T> stdout::Host for WasiCliImpl<T> where T: WasiCliView {}

impl<T> stderr::HostConcurrent for WasiCli<T>
where
    T: WasiCliView + 'static,
{
    async fn set_stderr<U: 'static>(
        store: &mut Accessor<U, Self>,
        data: HostStream<u8>,
    ) -> wasmtime::Result<()> {
        store.with(|mut view| {
            let stderr = view.get().cli().stderr.writer();
            let data = data.into_reader(&mut view);
            view.spawn(OutputTask {
                output: stderr,
                data,
            });
            Ok(())
        })
    }
}

impl<T> stderr::Host for WasiCliImpl<T> where T: WasiCliView {}

impl<T> environment::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_environment(&mut self) -> wasmtime::Result<Vec<(String, String)>> {
        Ok(self.cli().environment.clone())
    }

    fn get_arguments(&mut self) -> wasmtime::Result<Vec<String>> {
        Ok(self.cli().arguments.clone())
    }

    fn initial_cwd(&mut self) -> wasmtime::Result<Option<String>> {
        Ok(self.cli().initial_cwd.clone())
    }
}

impl<T> exit::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn exit(&mut self, status: Result<(), ()>) -> wasmtime::Result<()> {
        let status = match status {
            Ok(()) => 0,
            Err(()) => 1,
        };
        Err(anyhow!(I32Exit(status)))
    }

    fn exit_with_code(&mut self, status_code: u8) -> wasmtime::Result<()> {
        Err(anyhow!(I32Exit(status_code.into())))
    }
}
