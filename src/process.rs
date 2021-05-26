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

// [[file:../vasp-tools.note::*stdout][stdout:2]]
use tokio::io;
use tokio::process::{ChildStdin, ChildStdout};

type ReturnOutput = tokio::sync::mpsc::Sender<String>;
type ReadOutput = tokio::sync::mpsc::Receiver<String>;
type WriteInput = tokio::sync::mpsc::Receiver<String>;

pub struct StdoutReader {
    reader: tokio::io::Lines<io::BufReader<ChildStdout>>,
}

impl StdoutReader {
    pub fn new(stdout: ChildStdout) -> Self {
        use io::AsyncBufReadExt;

        let reader = io::BufReader::new(stdout).lines();
        Self { reader }
    }

    /// Read stdout until finding a line containing the `pattern`
    pub async fn read_until(&mut self, pattern: &str) -> Result<String> {
        use io::AsyncBufRead;

        let mut text = String::new();
        while let Some(line) = self.reader.next_line().await? {
            writeln!(&mut text, "{}", line)?;
            if dbg!(line).contains(&pattern) {
                return Ok(text);
            }
        }
        bail!("expected pattern not found!");
    }
}
// stdout:2 ends here

// [[file:../vasp-tools.note::*stdin][stdin:1]]
pub struct StdinWriter {
    stdin: ChildStdin,
}

impl StdinWriter {
    pub fn new(stdin: ChildStdin) -> Self {
        Self { stdin }
    }

    /// Write `input` into self's stdin
    pub async fn write(&mut self, input: &str) -> Result<()> {
        use io::AsyncWriteExt;

        self.stdin.write_all(dbg!(input).as_bytes()).await?;
        self.stdin.flush().await?;

        Ok(())
    }
}
// stdin:1 ends here

// [[file:../vasp-tools.note::*session][session:1]]
mod session {
    use gut::prelude::*;
    use std::process::ExitStatus;
    use tokio::process::{Child, ChildStdin, ChildStdout, Command};
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    /// Manage process session
    #[derive(Debug)]
    struct Session {
        command: Command,
        stdin: Option<ChildStdin>,
        stdout: Option<ChildStdout>,
        run_handler: Option<JoinHandle<Option<ExitStatus>>>,
        // session id
        id: Option<u32>,
    }

    impl Session {
        fn new(command: Command) -> Self {
            Self {
                command,
                id: None,
                stdin: None,
                stdout: None,
                run_handler: None,
            }
        }

        fn spawn(&mut self) -> Result<()> {
            use crate::process::ProcessGroupExt;
            use std::process::Stdio;

            // we want to interact with child process's stdin and stdout
            let mut child = self
                .command
                .new_process_group()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;

            self.stdin = child.stdin.take();
            self.stdout = child.stdout.take();
            self.id = child.id();

            let h = tokio::spawn(async move {
                let status = child.wait().await.ok()?;
                Some(status)
            });
            self.run_handler = Some(h);

            Ok(())
        }

        async fn write_stdin(&mut self, input: &str) -> Result<()> {
            todo!()
        }

        async fn read_stdout_until_matching_line(&mut self, pattern: &str) -> Result<()> {
            todo!()
        }

        pub async fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
            todo!()
        }
    }

    // #[tokio::test]
    // async fn test_session_communicate() -> Result<()> {
    //     let mut s = Session::new()?;
    //     let o = s.interact("", "next line").await?;

    //     Ok(())
    // }
}
// session:1 ends here
