// [[file:../../vasp-tools.note::*imports][imports:1]]
use gut::cli::*;
use gut::prelude::*;
use structopt::*;

use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../../vasp-tools.note::*main][main:1]]
use structopt::*;

/// A helper program for run VASP calculations
#[derive(Debug, StructOpt)]
struct Cli {
    #[structopt(flatten)]
    verbose: gut::cli::Verbosity,

    /// The command to invoke VASP program, e.g. "mpirun -np 16 vasp"
    #[structopt(short = "x")]
    program: String,

    /// Path to the socket file to bind. This option implies interactive VASP
    /// calculation.
    #[structopt(short = "u")]
    socket_file: Option<PathBuf>,
}

fn main() {
    todo!();
}
// main:1 ends here
