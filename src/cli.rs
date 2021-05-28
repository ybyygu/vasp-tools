// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::fs::*;
use gut::prelude::*;
use std::path::PathBuf;
use structopt::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*vasp][vasp:1]]
// just for test
fn interactive_vasp_session_bbm(mut session: crate::session::Session) -> Result<()> {
    // for the first time run, VASP reads coordinates from POSCAR
    let input: String = if !std::path::Path::new("OUTCAR").exists() {
        info!("Write complete POSCAR file for initial calculation.");
        let txt = crate::vasp::stdin::read_txt_from_stdin()?;
        gut::fs::write_to_file("POSCAR", &txt)?;
        // inform server to start with empty input
        "".into()
    } else {
        // redirect scaled positions to server for interactive VASP calculations
        info!("Send scaled coordinates to interactive VASP server.");
        crate::vasp::stdin::get_scaled_positions_from_stdin()?
    };

    // wait for output
    let s = session.interact(&input, VASP_READ_PATTERN)?;

    let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&s)?;
    let mut mp = gosh::model::ModelProperties::default();
    mp.set_energy(energy);
    mp.set_forces(forces);
    println!("{}", mp);

    Ok(())
}
// vasp:1 ends here

// [[file:../vasp-tools.note::*server][server:1]]
// NOTE: The read pattern is different
// VASP 5.3.5: "POSITIONS: read from stdin";
// VASP 6.1.0: "POSITIONS: reading from stdin";
// const VASP_READ_PATTERN: &str = "POSITIONS: read from stdin";
// const VASP_READ_PATTERN: &str = "POSITIONS: reading from stdin";
const VASP_READ_PATTERN: &str = "POSITIONS: read";

/// A helper program for run VASP calculations
#[derive(Debug, StructOpt)]
struct ServerCli {
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

    #[structopt(long)]
    adhoc: bool,

    /// Path to the socket file to bind (only valid for interactive calculation)
    #[structopt(short = "u", default_value = "vasp.sock")]
    socket_file: PathBuf,
}

#[tokio::main]
pub async fn run_vasp_enter_main() -> Result<()> {
    let args = ServerCli::from_args();
    args.verbose.setup_logger();

    let vasp_program = &args.program;
    let interactive = args.interactive;

    // adhoc hacking
    if args.adhoc {
        let session = crate::session::new_session(&args.program);
        interactive_vasp_session_bbm(session)?;

        return Ok(());
    }

    if interactive {
        info!("Run VASP for interactive calculation ...");
        crate::vasp::update_incar_for_bbm(interactive)?;
        crate::socket::Server::create(&args.socket_file)?
            .run_and_serve(vasp_program)
            .await;
    } else {
        info!("Run VASP for one time single-point calculation ...");
        crate::vasp::update_incar_for_bbm(false)?;
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

        // or we can use `std::process::Command` directly
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
// server:1 ends here

// [[file:../vasp-tools.note::*client][client:1]]
/// A client of a unix domain socket server for interacting with the program
/// run in background
#[derive(Debug, StructOpt)]
struct ClientCli {
    #[structopt(flatten)]
    verbose: gut::cli::Verbosity,

    /// Path to the socket file to connect
    #[structopt(short = "u", default_value = "vasp.sock")]
    socket_file: PathBuf,

    /// Stop VASP server
    #[structopt(short = "q")]
    quit: bool,
}

#[tokio::main]
pub async fn vasp_client_enter_main() -> Result<()> {
    use crate::socket::Client;
    use gosh::model::ModelProperties;

    let args = ClientCli::from_args();
    args.verbose.setup_logger();

    let mut client = Client::connect(&args.socket_file).await?;

    if args.quit {
        client.try_quit().await?;
        return Ok(());
    }

    // for the first time run, VASP reads coordinates from POSCAR
    let input: String = if !std::path::Path::new("OUTCAR").exists() {
        info!("Write complete POSCAR file for initial calculation.");
        let txt = crate::vasp::stdin::read_txt_from_stdin()?;
        gut::fs::write_to_file("POSCAR", &txt)?;
        // inform server to start with empty input
        "".into()
    } else {
        // resume paused calculation
        client.try_resume().await?;
        // redirect scaled positions to server for interactive VASP calculations
        info!("Send scaled coordinates to interactive VASP server.");
        crate::vasp::stdin::get_scaled_positions_from_stdin()?
    };

    // wait for output
    let s = client.interact(&input, VASP_READ_PATTERN).await?;
    let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&s)?;
    let mut mp = gosh::model::ModelProperties::default();
    mp.set_energy(energy);
    mp.set_forces(forces);
    println!("{}", mp);

    // pause VASP to avoid wasting CPU times, which will be resumed on next calculation
    client.try_pause().await?;

    Ok(())
}
// client:1 ends here

// [[file:../vasp-tools.note::*simulate][simulate:1]]
pub fn simulate_interactive_vasp() -> Result<()> {
    let part0 = include_str!("../tests/files/interactive_iter0.txt");
    let part1 = include_str!("../tests/files/interactive_iter1.txt");
    let energy = "F= -.85097948E+02 E0= -.85096866E+02  d E =-.850979E+02  mag=     2.9646";
    let i = 4;

    let natoms = 25;
    let stdin = std::io::stdin();
    print!("{}", part0);
    for i in 2.. {
        println!("POSITIONS: reading from stdin");
        let mut handler = stdin.lock();
        let mut positions = String::new();
        for _ in 0..natoms {
            handler.read_line(&mut positions)?;
        }
        print!("{}", part1);
        println!("{:4} {}", i, energy);
    }
    Ok(())
}
// simulate:1 ends here
