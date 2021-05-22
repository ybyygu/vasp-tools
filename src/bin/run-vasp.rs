// [[file:../../vasp-tools.note::*imports][imports:1]]
use gut::cli::*;
use gut::prelude::*;
use structopt::*;

use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../../vasp-tools.note::*main][main:1]]
use gut::prelude::*;

fn main() -> Result<()> {
    vasp_tools::run_vasp_enter_main()?;

    Ok(())
}
// main:1 ends here
