// [[file:../../vasp-tools.note::*bin/fake-vasp.rs][bin/fake-vasp.rs:1]]
use gut::prelude::*;

fn main() -> Result<()> {
    vasp_tools::simulate_interactive_vasp()?;

    Ok(())
}
// bin/fake-vasp.rs:1 ends here
