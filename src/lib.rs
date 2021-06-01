// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod cli;
mod interactive;
mod process;
mod session;
mod socket;
mod vasp;

pub(crate) mod common {
    pub use gut::prelude::*;
    pub use std::path::{Path, PathBuf};

    /// Sleep a few seconds
    pub fn sleep(t: f64) {
        std::thread::sleep(std::time::Duration::from_secs_f64(t));
    }

    /// Wait until file `f` available for max time of `timeout`.
    ///
    /// # Parameters
    /// * timeout: timeout in seconds
    /// * f: the file to wait for available
    pub fn wait_file(f: &Path, timeout: usize) -> Result<()> {
        // wait a moment for socke file ready
        let interval = 0.1;
        let mut t = 0.0;
        loop {
            if f.exists() {
                info!("Elapsed time during waiting: {:.2} seconds ", t);
                return Ok(());
            }
            t += interval;
            sleep(interval);

            if t > timeout as f64 {
                bail!("file {:?} doest exist for {} seconds", f, timeout);
            }
        }
    }
}
// mods:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use crate::cli::*;
// pub:1 ends here
