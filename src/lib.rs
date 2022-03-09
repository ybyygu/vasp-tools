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
mod session;
mod socket;
mod vasp;
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
