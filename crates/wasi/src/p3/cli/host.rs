use anyhow::{anyhow, Context as _};
use wasmtime::component::{stream, Accessor, Resource, StreamReader};

use crate::p3::bindings::cli::{
    environment, exit, stderr, stdin, stdout, terminal_input, terminal_output, terminal_stderr,
    terminal_stdin, terminal_stdout,
};
use crate::p3::cli::{I32Exit, TerminalInput, TerminalOutput, WasiCliImpl, WasiCliView};
use crate::p3::ResourceView as _;

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
    async fn get_stdin<U>(store: &mut Accessor<U, Self>) -> wasmtime::Result<StreamReader<u8>> {
        // TODO: Implement
        store.with(|mut view| {
            let (tx, rx) = stream(&mut view).context("failed to create stream")?;
            tx.close(&mut view).context("failed to close stream")?;
            Ok(rx)
        })
    }
}

impl<T> stdout::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn set_stdout(&mut self, _data: StreamReader<u8>) -> wasmtime::Result<()> {
        // TODO: Implement
        Ok(())
    }
}

impl<T> stderr::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn set_stderr(&mut self, _data: StreamReader<u8>) -> wasmtime::Result<()> {
        // TODO: Implement
        Ok(())
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
