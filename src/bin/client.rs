// [[file:../../vasp-server.note::*bin/client.rs][bin/client.rs:1]]
use gut::prelude::*;

fn main() -> Result<()> {
    vasp_server::client_enter_main()?;

    Ok(())
}
// bin/client.rs:1 ends here