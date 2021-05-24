// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod interactive;
mod process;
mod socket;
mod vasp;
// mods:1 ends here

// [[file:../vasp-tools.note::*cli][cli:1]]
mod cli {
    use gut::prelude::*;
    use std::path::PathBuf;
    use structopt::*;

    /// A helper program for run VASP calculations
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// The command or the path to invoking VASP program
        #[structopt(short = "x")]
        program: PathBuf,

        /// Run VASP for one-time single point calculation. The mandatory
        /// parameters in INCAR will be automatically updated.
        #[structopt(long, conflicts_with = "interactive")]
        single_point: bool,

        /// Run VASP in interactive mode for long-live calculation. The
        /// mandatory parameters in INCAR will be automatically updated.
        #[structopt(long, conflicts_with = "single_point")]
        interactive: bool,

        /// Path to the socket file to bind (only valid for interactive calculation)
        #[structopt(short = "u", default_value = "vasp.sock")]
        socket_file: PathBuf,
    }

    pub fn run_vasp_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        crate::vasp::update_incar_for_bbm(args.interactive)?;

        let vasp_program = &args.program;
        if args.interactive {
            crate::socket::Server::create(&args.socket_file)?.run_and_serve(vasp_program)?;
        } else {
            // NOTE: we need handle duct::IntoExecutablePath trick. In duct
            // crate, the Path has different semantics with `String`: a program
            // registered under PATH env var or the path (relative or full) to
            // the program file?
            let _cmd = vasp_program.to_string_lossy();
            if _cmd.contains("/") {
                duct::cmd!(vasp_program)
            } else {
                duct::cmd!(_cmd.into_owned())
            }
            .unchecked()
            .run()
            .with_context(|| format!("Run VASP failure using {:?}", vasp_program))?;

            // use Command in std lib?
            //
            // if let Err(e) = std::process::Command::new(vasp_program)
            //     .spawn()
            //     .with_context(|| format!("run vasp program: {:?}", vasp_program))?
            //     .wait()
            // {
            //     error!("wait vasp process error: {:?}", e);
            // }
        }

        Ok(())
    }
}
// cli:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use crate::cli::run_vasp_enter_main;
pub use crate::socket::client_enter_main;
// pub:1 ends here
