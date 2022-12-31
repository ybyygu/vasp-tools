// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::path::{Path, PathBuf};

#[macro_use]
extern crate approx; // For the macro relative_eq!
                     // imports:1 ends here

// [[file:../vasp-tools.note::a397a097][a397a097]]
pub mod cli;
mod interactive;
mod plot;
mod socket;
mod vasp;

mod session {
    use super::*;

    pub use gosh::runner::interactive::InteractiveSession as Session;
    pub use gosh::runner::process::SessionHandler;

    #[test]
    fn test_interactive_vasp() -> Result<()> {
        let read_pattern = "POSITIONS: reading from stdin";

        // the input for writing into stdin
        let positions = include_str!("../tests/files/interactive_positions.txt");

        let vasp = std::process::Command::new("fake-vasp");
        let mut s = Session::new(vasp);
        let h = s.spawn()?;

        let o = s.interact("", read_pattern)?;
        let _ = crate::vasp::stdout::parse_energy_and_forces(&o)?;
        let o = s.interact(&positions, read_pattern)?;
        let (energy2, _forces2) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
        assert_eq!(energy2, 2.0);
        let o = s.interact(&positions, read_pattern)?;
        let (energy3, _forces3) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
        assert_eq!(energy3, 3.0);

        h.terminate()?;

        Ok(())
    }
}
// a397a097 ends here

// [[file:../vasp-tools.note::57018756][57018756]]
use gut::prelude::*;

use crate::session::*;
use crate::vasp::VaspOutcar;

/// Wait until file `f` available for max time of `timeout`.
///
/// # Parameters
/// * timeout: timeout in seconds
/// * f: the file to wait for available
fn wait_file(f: &Path, timeout: usize) -> Result<()> {
    use gut::utils::sleep;

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
// 57018756 ends here

// [[file:../vasp-tools.note::242ad86a][242ad86a]]
#[cfg(feature = "adhoc")]
/// Docs for local mods
pub mod docs {
    macro_rules! export_doc {
        ($l:ident) => {
            pub mod $l {
                pub use crate::$l::*;
            }
        };
    }

    export_doc!(interactive);
    export_doc!(session);
    export_doc!(socket);
    export_doc!(vasp);
    export_doc!(plot);
}
// 242ad86a ends here
