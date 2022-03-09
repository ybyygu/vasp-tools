// [[file:../../vasp-tools.note::be828118][be828118]]
use gut::prelude::*;

fn main() -> Result<()> {
    vasp_tools::cli::simulate_interactive_vasp()?;

    Ok(())
}
// be828118 ends here
