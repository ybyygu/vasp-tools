// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use nix::unistd::Pid;
// imports:1 ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod interactive;
mod socket;
mod vasp;
// mods:1 ends here

// [[file:../vasp-tools.note::*pidfile][pidfile:1]]
#[derive(Debug)]
pub struct PidFile {
    file: std::fs::File,
    path: PathBuf,
}

impl PidFile {
    // 生成vasp.pid文件, 如果vasp server已运行, 将报错
    fn create<P: AsRef<Path>>(path: P) -> Result<PidFile> {
        use fs2::*;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .context("Could not create PID file")?;

        // https://docs.rs/fs2/0.4.3/fs2/trait.FileExt.html
        file.try_lock_exclusive()
            .context("Could not lock PID file; Is the daemon already running?")?;

        Ok(PidFile {
            file,
            path: path.as_ref().to_owned(),
        })
    }

    // 写入daemon进程号, 方便外部控制
    fn write_pid(&mut self, pid: Pid) -> Result<()> {
        writeln!(&mut self.file, "{}", pid).context("Could not write PID file")?;
        self.file.flush().context("Could not flush PID file")
    }
}

impl Drop for PidFile {
    // daemon退出时, 清理pidfile
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
// pidfile:1 ends here

// [[file:../vasp-tools.note::*daemon][daemon:1]]
#[cfg(unix)]
pub fn daemonize() -> Result<PidFile> {
    // use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixListener;

    // https://man7.org/linux/man-pages/man3/daemon.3.html
    // 根据Linux API手册, 这里不执行double fork, 因此该daemon是session leader
    // the resulting daemon is a session leader
    nix::unistd::daemon(true, true)?;

    // 生成进程session, 方便退出时处理子进程
    // if nix::unistd::setsid().is_err() {
    //     eprintln!("Could not create new session");
    // }
    let sid = std::process::id() as i32;
    println!(" my pid {}", sid);

    let mut pid_file = PidFile::create("vasp-server.pid")?;
    pid_file.write_pid(Pid::from_raw(sid))?;

    let console_sock: PathBuf = "vasp-server.sock".into();

    // https://stackoverflow.com/questions/40218416/how-do-i-close-a-unix-socket-in-rust
    // servers should unlink the socket pathname prior to binding it.
    let listener = UnixListener::bind(&console_sock)?;
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut msg = String::new();
        let n = stream.read_to_string(&mut msg)?;
        dbg!(n);
        let action = msg.trim();
        if dbg!(action) == "exit" {
            break;
        }
    }

    Ok(pid_file)
}
// daemon:1 ends here

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

        /// The command to invoke VASP program
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
            // FIXME: cannot find vasp535 program, why?
            // duct::cmd!(vasp_program)
            //     .unchecked()
            //     .run()
            //     .with_context(|| format!("run {:?} failure", vasp_program))?;

            if let Err(e) = std::process::Command::new(vasp_program)
                .spawn()
                .with_context(|| format!("run vasp program: {:?}", vasp_program))?
                .wait()
            {
                error!("wait vasp process error: {:?}", e);
            }
        }

        Ok(())
    }
}
// cli:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use crate::cli::run_vasp_enter_main;
pub use crate::socket::{client_enter_main, server_enter_main};
// pub:1 ends here
