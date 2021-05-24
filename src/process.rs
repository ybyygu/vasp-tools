// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::path::{Path, PathBuf};

use std::io::prelude::*;
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

pub trait ProcessGroupExt<T> {
    fn new_process_group(&mut self) -> &mut T;
}

use std::process::Command;
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

// [[file:../vasp-tools.note::*process handler][process handler:1]]
use shared_child::SharedChild;

use shared_child::unix::SharedChildExt;
// use futures::channel::oneshot::channel;
// use std::time::Duration;

#[derive(Clone)]
pub struct ProcessHandle {
    process: std::sync::Arc<SharedChild>,
}

impl ProcessHandle {
    pub fn new(mut command: &mut Command) -> Result<ProcessHandle> {
        Ok(ProcessHandle {
            process: std::sync::Arc::new(SharedChild::spawn(&mut command)?),
        })
    }

    /// Kill child process
    pub fn kill(&self) {
        let _ = self.process.kill();
    }

    /// Return child process id
    pub fn pid(&self) -> u32 {
        self.process.id()
    }

    /// Check if child process still running
    pub fn check_if_running(&self) -> Result<()> {
        let pid = self.pid();

        let status = self
            .process
            .try_wait()
            .with_context(|| format!("Failed to wait for process {:?}", pid))?;
        let _ = status.ok_or(format_err!("Process [pid={}] is still running.", pid))?;

        Ok(())
    }

    pub fn terminate(&self) -> Result<()> {
        self.process.send_signal(libc::SIGTERM)?;
        // Error means, that probably process was already terminated, because:
        // - We have permissions to send signal, since we created this process.
        // - We specified correct signal SIGTERM.
        // But better let's check.
        self.check_if_running()?;

        Ok(())
    }

    pub fn wait_until_finished(self) -> Result<()> {
        // On another thread, wait on the child process.
        let child_arc_clone = self.process.clone();
        let thread = std::thread::spawn(move || child_arc_clone.wait().unwrap());

        // let process = self.process.clone();
        // let (sender, receiver) = channel::<ExeUnitExitStatus>();

        // std::thread::spawn(move || {
        //     let result = process.wait();

        //     let status = match result {
        //         Ok(status) => match status.code() {
        //             // status.code() will return None in case of termination by signal.
        //             None => ExeUnitExitStatus::Aborted(status),
        //             Some(_code) => ExeUnitExitStatus::Finished(status),
        //         },
        //         Err(error) => ExeUnitExitStatus::Error(error),
        //     };
        //     sender.send(status)
        // });

        // Note: unwrap can't fail here. All sender, receiver and thread will
        // end their lifetime before await will return. There's no danger
        // that one of them will be dropped earlier.
        // return receiver.await.unwrap();

        Ok(())
    }
}
// process handler:1 ends here
