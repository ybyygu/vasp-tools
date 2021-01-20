// [[file:../../vasp-server.note::*bin/vasp-adhoc-control.rs][bin/vasp-adhoc-control.rs:1]]
use gut::prelude::*;

fn main() -> Result<()> {
    vasp_server::adhoc::enter_main()?;

    Ok(())
}
// bin/vasp-adhoc-control.rs:1 ends here
