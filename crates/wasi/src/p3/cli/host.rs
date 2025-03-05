use anyhow::{anyhow, Context as _};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use wasmtime::component::{
    stream, Accessor, AccessorTask, HostStream, Resource, StreamReader, StreamWriter,
};

use crate::p3::bindings::cli::{
    environment, exit, stderr, stdin, stdout, terminal_input, terminal_output, terminal_stderr,
    terminal_stdin, terminal_stdout,
};
use crate::p3::cli::{I32Exit, TerminalInput, TerminalOutput, WasiCliImpl, WasiCliView};
use crate::p3::ResourceView as _;

struct InputTask<T> {
    input: T,
    tx: StreamWriter<Vec<u8>>,
}

impl<T, U, V> AccessorTask<T, U, wasmtime::Result<()>> for InputTask<V>
where
    V: AsyncRead + Send + Sync + Unpin + 'static,
{
    async fn run(mut self, _: &mut Accessor<T, U>) -> wasmtime::Result<()> {
        let mut tx = self.tx;
        loop {
            let mut buf = vec![0; 8096];
            match self.input.read(&mut buf).await {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    buf.truncate(n);
                    let Some(tail) = tx.write(buf).into_future().await else {
                        break Ok(());
                    };
                    tx = tail;
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
    data: StreamReader<Vec<u8>>,
}

impl<T, U, V> AccessorTask<T, U, wasmtime::Result<()>> for OutputTask<V>
where
    V: AsyncWrite + Send + Sync + Unpin + 'static,
{
    async fn run(mut self, _: &mut Accessor<T, U>) -> wasmtime::Result<()> {
        let mut fut = self.data.read().into_future();
        'outer: loop {
            let Ok((tail, buf)) = fut.await else {
                return Ok(());
            };
            let mut buf = buf.as_slice();
            loop {
                match self.output.write(&buf).await {
                    Ok(n) => {
                        if n == buf.len() {
                            fut = tail.read().into_future();
                            continue 'outer;
                        } else {
                            buf = &buf[n..];
                        }
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

impl<T> stdin::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    async fn get_stdin<U: 'static>(
        store: &mut Accessor<U, Self>,
    ) -> wasmtime::Result<HostStream<u8>> {
        store.with(|mut view| {
            let (tx, rx) = stream(&mut view).context("failed to create stream")?;
            let stdin = view.cli().stdin.reader();
            view.spawn(InputTask { input: stdin, tx });
            Ok(rx.into())
        })
    }
}

impl<T> stdout::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    async fn set_stdout<U: 'static>(
        store: &mut Accessor<U, Self>,
        data: HostStream<u8>,
    ) -> wasmtime::Result<()> {
        store.with(|mut view| {
            let stdout = view.cli().stdout.writer();
            let data = data.into_reader(&mut view);
            view.spawn(OutputTask {
                output: stdout,
                data,
            });
            Ok(())
        })
    }
}

impl<T> stderr::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    async fn set_stderr<U: 'static>(
        store: &mut Accessor<U, Self>,
        data: HostStream<u8>,
    ) -> wasmtime::Result<()> {
        store.with(|mut view| {
            let stderr = view.cli().stderr.writer();
            let data = data.into_reader(&mut view);
            view.spawn(OutputTask {
                output: stderr,
                data,
            });
            Ok(())
        })
    }
}

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
