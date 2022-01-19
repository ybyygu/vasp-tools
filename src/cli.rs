// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
use crate::socket::Client;

use gosh::model::ModelProperties;
use gut::fs::*;
use structopt::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*vasp][vasp:1]]
// const VASP_READ_PATTERN: &str = "POSITIONS: read from stdin";
const VASP_READ_PATTERN: &str = "POSITIONS: reading from stdin";

/// # Parameters
///
/// * control: try to pause/resume running process to reduce CPU usages
async fn interactive_vasp_session_bbm(client: &mut Client, control: bool) -> Result<()> {
    use gosh::adaptor::ModelAdaptor;
    
    // for the first time run, VASP reads coordinates from POSCAR
    let input: String = if !std::path::Path::new("OUTCAR").exists() {
        debug!("Write complete POSCAR file for initial calculation.");
        let txt = crate::vasp::stdin::read_txt_from_stdin()?;
        gut::fs::write_to_file("POSCAR", &txt)?;
        // inform server to start with empty input
        "".into()
    } else {
        // resume paused calculation
        if control {
            client.try_resume().await?;
        }
        // redirect scaled positions to server for interactive VASP calculationsSP
        debug!("Send scaled coordinates to interactive VASP server.");
        crate::vasp::stdin::get_scaled_positions_from_stdin()?
    };

    // wait for output
    let s = client.interact(&input, VASP_READ_PATTERN).await?;
    // NOTE: for larger system, there may have no energy/forces information in
    // stdout
    // let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&s)?;
    // let mut mp = ModelProperties::default();
    // mp.set_energy(energy);
    // mp.set_forces(forces);
    let mp = gosh::adaptor::Vasp().parse_last("OUTCAR")?;
    println!("{}", mp);

    // pause VASP to avoid wasting CPU times, which will be resumed on next calculation
    if control {
        client.try_pause().await?;
    }

    Ok(())
}

/// for creating `fake-vasp` binary, simulating interactive VASP caclulation
pub fn simulate_interactive_vasp() -> Result<()> {
    let part0 = include_str!("../tests/files/interactive_iter0.txt");
    let part1 = include_str!("../tests/files/interactive_iter1.txt");
    // let energy = "F= -.85097948E+02 E0= -.85096866E+02  d E =-.850979E+02  mag=     2.9646";

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
        // make it slower: 0.1 second delay
        sleep(0.1);
        print!("{}", part1);
        // replace energy with iter number, so we can test against it to make
        // sure we are correct during multiple interactions.
        let energy = format!("F= -.85097948E+02 E0={:-12.8E}  d E =-.850979E+02  mag=     2.9646", i);
        println!("{:4} {}", i, energy);
    }
    Ok(())
}
// vasp:1 ends here

// [[file:../vasp-tools.note::79d54340][79d54340]]
/// A helper program for run VASP calculations
#[derive(Debug, StructOpt)]
struct ServerCli {
    #[structopt(flatten)]
    verbose: gut::cli::Verbosity,

    /// The command or the path to invoking VASP program. If not provided, only
    /// the INCAR will be updated.
    #[structopt(short = "x")]
    program: Option<PathBuf>,

    /// Instruct VASP to stop by writing a STOPCAR file in working directory.
    #[structopt(long, name = "VASP_WORK_DIR")]
    stop: Option<PathBuf>,

    /// Run VASP for one-time single point calculation. The mandatory
    /// parameters in INCAR will be automatically updated.
    #[structopt(long, conflicts_with = "interactive, frequency")]
    single_point: bool,

    /// Run VASP for frequency calculation. The mandatory parameters in INCAR
    /// will be automatically updated.
    #[structopt(long, conflicts_with = "interactive, single_point")]
    frequency: bool,

    /// Run VASP in interactive mode for long-live calculation. The
    /// mandatory parameters in INCAR will be automatically updated.
    #[structopt(long, conflicts_with = "single_point")]
    interactive: bool,

    /// Run VASP for static magnetic configuration calculation. The mandatory
    /// parameters in INCAR will be automatically updated.
    #[structopt(long, conflicts_with = "interactive, frequency, single_point")]
    magnetic: Option<String>,

    /// Path to the socket file to bind (only valid for interactive calculation)
    #[structopt(short = "u", default_value = "vasp.sock")]
    socket_file: PathBuf,
}

#[tokio::main]
pub async fn run_vasp_enter_main() -> Result<()> {
    use crate::vasp::VaspTask;

    let args = ServerCli::from_args();
    args.verbose.setup_logger();

    // write STOPCAR only
    if let Some(wrk_dir) = &args.stop {
        crate::vasp::stopcar::write(wrk_dir)?;
        return Ok(());
    }

    let vasp_program = &args.program;
    let interactive = args.interactive;

    if interactive {
        crate::vasp::update_incar_for_bbm(&VaspTask::Interactive)?;
        if let Some(vasp_program) = &args.program {
            debug!("Run VASP for interactive calculation ...");
            crate::socket::Server::create(&args.socket_file)?
                .run_and_serve(vasp_program)
                .await;
        }
    } else {
        let task = if args.single_point {
            VaspTask::SinglePoint
        } else if args.frequency {
            VaspTask::Frequency
        } else {
            if let Some(mag) = args.magnetic {
                VaspTask::Magnetic(mag)
            } else {
                ServerCli::clap().print_help();
                return Ok(());
            }
        };
        crate::vasp::update_incar_for_bbm(&task)?;
        if let Some(vasp_program) = &args.program {
            debug!("Run VASP for {:?} calculation ...", task);
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
    }

    Ok(())
}
// 79d54340 ends here

// [[file:../vasp-tools.note::28b92274][28b92274]]
/// A client of a unix domain socket server for interacting with the program
/// run in background
#[derive(Debug, StructOpt)]
struct ClientCli {
    #[structopt(flatten)]
    verbose: gut::cli::Verbosity,

    /// Path to the socket file to connect
    #[structopt(short = "u", default_value = "vasp.sock")]
    socket_file: PathBuf,

    /// Control child process for saving CPU times when idle
    #[structopt(long)]
    control: bool,

    /// Stop VASP server
    #[structopt(short = "q")]
    quit: bool,
}

#[tokio::main]
pub async fn vasp_client_enter_main() -> Result<()> {
    let args = ClientCli::from_args();
    args.verbose.setup_logger();

    // wait a moment for socke file ready
    let timeout = 5;
    wait_file(&args.socket_file, timeout)?;
    let mut client = Client::connect(&args.socket_file).await?;

    if args.quit {
        client.try_quit().await?;
        return Ok(());
    }

    interactive_vasp_session_bbm(&mut client, args.control).await?;

    Ok(())
}
// 28b92274 ends here

// [[file:../vasp-tools.note::*vibrational mode][vibrational mode:1]]
/// A helper program for run VASP calculations
#[derive(Debug, StructOpt)]
struct VibCli {
    #[structopt(flatten)]
    verbose: gut::cli::Verbosity,

    /// Extract last imaginary frequency mode
    #[structopt(long, name = "OUTCAR")]
    extract_vib_mode: PathBuf,

    /// Run VASP for frequency calculation. The mandatory parameters in INCAR
    /// will be automatically updated.
    #[structopt(long, conflicts_with = "interactive, single_point")]
    frequency: bool,

    /// The output file for writing vibrational mode
    #[structopt(short = "o")]
    outfile: PathBuf,
}

pub fn vib_mode_enter_main() -> Result<()> {
    let args = VibCli::from_args();
    args.verbose.setup_logger();

    let outcar = &args.extract_vib_mode;
    let mode = crate::vasp::VaspOutcar::parse_last_imaginary_freq_mode_from(outcar)?;
    let s: String = mode
        .into_iter()
        .map(|x| format!("{:-18.6} {:-18.6} {:-18.6}\n", x[0], x[1], x[2]))
        .collect();
    gut::fs::write_to_file(&args.outfile, &s)?;

    Ok(())
}
// vibrational mode:1 ends here

// [[file:../vasp-tools.note::3fdb5cf5][3fdb5cf5]]
#[derive(Debug, StructOpt)]
/// Show a summary on VASP OUTCAR
struct SummaryCli {
    #[structopt(flatten)]
    verbose: gut::cli::Verbosity,

    /// Show a plot on optimization.
    #[structopt(long)]
    plot: bool,
}

pub fn vasp_summary_enter_main() -> Result<()> {
    let args = SummaryCli::from_args();
    args.verbose.setup_logger();

    crate::vasp::outcar::summarize_outcar("OUTCAR".as_ref(), args.plot)?;
    Ok(())
}
// 3fdb5cf5 ends here
