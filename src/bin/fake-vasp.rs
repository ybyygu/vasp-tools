// [[file:../../vasp-tools.note::*bin/run-vasp.rs][bin/run-vasp.rs:1]]
use gut::prelude::*;

fn main() -> Result<()> {
    vasp_tools::simulate_interactive_vasp()?;

    Ok(())
}
// bin/run-vasp.rs:1 ends here
