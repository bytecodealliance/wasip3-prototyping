use crate::p2::bindings::cli::environment;
use crate::p2::{WasiP2Impl, WasiP2View};

impl<T> environment::Host for WasiP2Impl<T>
where
    T: WasiP2View,
{
    fn get_environment(&mut self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(self.ctx().env.clone())
    }
    fn get_arguments(&mut self) -> anyhow::Result<Vec<String>> {
        Ok(self.ctx().args.clone())
    }
    fn initial_cwd(&mut self) -> anyhow::Result<Option<String>> {
        // FIXME: expose cwd in builder and save in ctx
        Ok(None)
    }
}
