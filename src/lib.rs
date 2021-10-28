// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::path::{Path, PathBuf};

#[macro_use]
extern crate approx; // For the macro relative_eq!
// imports:1 ends here

// [[file:../vasp-tools.note::a397a097][a397a097]]
mod cli;
mod interactive;
mod ipi;
mod plot;
mod session;
mod socket;
mod vasp;

pub(crate) mod common {
    pub use gut::prelude::*;
    pub use gut::utils::sleep;
    pub use std::path::{Path, PathBuf};

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
                trace!("Elapsed time during waiting: {:.2} seconds ", t);
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
// a397a097 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use crate::cli::*;
pub use crate::session::*;

pub use crate::vasp::VaspOutcar;
// pub:1 ends here
