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
}
// mods:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use crate::cli::*;
// pub:1 ends here
