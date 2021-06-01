// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;

use gut::prelude::*;
use std::io::prelude::*;
use std::process::Command;
// imports:1 ends here

// [[file:../vasp-tools.note::*process group][process group:1]]
macro_rules! setsid {
    () => {{
        // Don't check the error of setsid because it fails if we're the
        // process leader already. We just forked so it shouldn't return
        // error, but ignore it anyway.
        nix::unistd::setsid().ok();

        Ok(())
    }};
}

/// Create child process in new session
pub trait ProcessGroupExt<T> {
    fn new_process_group(&mut self) -> &mut T;
}

impl ProcessGroupExt<Command> for Command {
    fn new_process_group(&mut self) -> &mut Command {
        use std::os::unix::process::CommandExt;

        unsafe {
            self.pre_exec(|| setsid!());
        }
        self
    }
}

impl ProcessGroupExt<tokio::process::Command> for tokio::process::Command {
    fn new_process_group(&mut self) -> &mut tokio::process::Command {
        unsafe {
            self.pre_exec(|| setsid!());
        }
        self
    }
}
// process group:1 ends here

// [[file:../vasp-tools.note::*pidfile][pidfile:1]]
use nix::unistd::Pid;

#[derive(Debug)]
pub struct PidFile {
    file: std::fs::File,
    path: PathBuf,
}

impl PidFile {
    fn create(path: &Path) -> Result<PidFile> {
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
            path: path.to_owned(),
        })
    }

    fn write_pid(&mut self, pid: Pid) -> Result<()> {
        writeln!(&mut self.file, "{}", pid).context("Could not write PID file")?;
        self.file.flush().context("Could not flush PID file")
    }

    /// Create a pidfile for process `pid`
    pub fn new(path: &Path, pid: u32) -> Result<Self> {
        let mut pidfile = Self::create(path)?;
        pidfile.write_pid(Pid::from_raw(pid as i32));

        Ok(pidfile)
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
// pidfile:1 ends here
