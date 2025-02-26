#![allow(unused)] // TODO: remove

use anyhow::Context as _;
use wasmtime::component::Resource;

use crate::p3::bindings::cli::terminal_input::TerminalInput;
use crate::p3::bindings::cli::terminal_output::TerminalOutput;
use crate::p3::bindings::cli::{
    environment, exit, stderr, stdin, stdout, terminal_input, terminal_output, terminal_stderr,
    terminal_stdin, terminal_stdout,
};
use crate::p3::cli::{WasiCliImpl, WasiCliView};
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
        todo!()
    }
}

impl<T> terminal_stdout::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_terminal_stdout(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        todo!()
    }
}

impl<T> terminal_stderr::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_terminal_stderr(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        todo!()
    }
}

impl<T> stdin::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn get_stdin(&mut self) -> wasmtime::Result<wasmtime::component::StreamReader<u8>> {
        todo!()
    }
}

impl<T> stdout::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn set_stdout(&mut self, data: wasmtime::component::StreamReader<u8>) -> wasmtime::Result<()> {
        todo!()
    }
}

impl<T> stderr::Host for WasiCliImpl<T>
where
    T: WasiCliView,
{
    fn set_stderr(&mut self, data: wasmtime::component::StreamReader<u8>) -> wasmtime::Result<()> {
        todo!()
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
        todo!()
    }

    fn exit_with_code(&mut self, status_code: u8) -> wasmtime::Result<()> {
        todo!()
    }
}
