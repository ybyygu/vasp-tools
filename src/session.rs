// [[file:../vasp-tools.note::*docs][docs:1]]
//! Run child processes in a new session group for easy to interact and control.
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
use crate::process::ProcessGroupExt;

use std::io::{BufRead, BufReader};
use std::process::Command;
use std::process::{Child, ChildStdin, ChildStdout};
// imports:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
/// Run child processes in a new session group for easy control
pub struct Session {
    command: Option<Command>,
    session: Option<Child>,
    stream0: Option<ChildStdin>,
    stream1: Option<std::io::Lines<BufReader<ChildStdout>>>,
}
// base:1 ends here

// [[file:../vasp-tools.note::*core][core:1]]
/// Spawn child process in a new session
fn create_new_session(mut command: Command) -> Result<Child> {
    use std::process::Stdio;

    // we want to interact with child process's stdin and stdout
    let child = command
        .new_process_group()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    Ok(child)
}

impl Session {
    /// Create a new session for running `command`
    pub fn new(command: Command) -> Self {
        Self {
            command: command.into(),
            session: None,
            stream0: None,
            stream1: None,
        }
    }

    fn quit(&mut self) -> Result<()> {
        if let Some(child) = self.session.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    info!("child process is still running, force to terminate ...");
                    let h = SessionHandler::new(child.id());
                    h.terminate()?;
                }
                Ok(Some(n)) => {
                    info!("child process exited with code: {}", n);
                }
                Err(e) => {
                    error!("failed to check child process'status: {:?}", e);
                }
            }
        }
        Ok(())
    }

    /// Interact with child process's stdin using `input` and return stdout
    /// read-in until the line matching `read_pattern`. The child process will
    /// be automatically spawned if necessary.
    pub fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
        use std::io::prelude::*;

        let s = self.session.as_mut().expect("rexpect session not started yet");

        // ignore interaction with empty input
        let stdin = self.stream0.as_mut().unwrap();
        if !input.is_empty() {
            trace!("send input for child process's stdin ({} bytes)", input.len());
            stdin.write_all(input.as_bytes())?;
            stdin.flush()?;
        }
        trace!("send read pattern for child process's stdout: {:?}", read_pattern);

        let mut txt = String::new();
        let stdout = self.stream1.as_mut().unwrap();
        for line in stdout {
            let line = line?;
            writeln!(&mut txt, "{}", line)?;
            if line.starts_with(read_pattern) {
                break;
            }
        }

        if txt.is_empty() {
            bail!("Got nothing for pattern: {}", read_pattern);
        }
        return Ok(txt);
    }

    /// Return child process's session ID, useful for killing all child
    /// processes using `pkill` command.
    pub fn id(&self) -> Option<u32> {
        self.session.as_ref().map(|s| s.id())
    }

    pub(crate) fn spawn_new(&mut self) -> Result<u32> {
        let command = self.command.take().unwrap();
        let mut child = create_new_session(command)?;
        self.stream0 = child.stdin.take().unwrap().into();
        let stdout = child.stdout.take().unwrap();
        self.stream1 = BufReader::new(stdout).lines().into();
        self.session = child.into();

        let pid = self.id().unwrap();
        info!("start child process in new session: {:?}", pid);
        Ok(pid)
    }
}

/// Call `pkill` to send signal to related processes
fn signal_processes_by_session_id(sid: u32, signal: &str) -> Result<()> {
    debug!("kill session {} using signal {:?}", sid, signal);
    duct::cmd!("pkill", "--signal", signal, "-s", sid.to_string())
        .unchecked()
        .run()?;

    Ok(())
}
// core:1 ends here

// [[file:../vasp-tools.note::*handler][handler:1]]
#[derive(Debug, Clone)]
/// Control progress group in session using external `pkill` command
pub(crate) struct SessionHandler {
    pid: u32,
}

impl SessionHandler {
    /// Create a SessionHandler for process `pid`
    pub fn new(pid: u32) -> Self {
        Self { pid }
    }
}

impl SessionHandler {
    /// send signal to child processes: SIGINT, SIGTERM, SIGCONT, SIGSTOP
    fn signal(&self, sig: &str) -> Result<()> {
        info!("signal process {} with {}", self.pid, sig);
        signal_processes_by_session_id(self.pid, sig)?;
        Ok(())
    }

    /// Terminate child processes in a session.
    pub fn terminate(&self) -> Result<()> {
        // If process was paused, terminate it directly could be deadlock
        self.signal("SIGCONT");
        sleep(0.2);
        self.signal("SIGTERM")
    }

    /// Kill processes in a session.
    pub fn kill(&self) -> Result<()> {
        self.signal("SIGKILL")
    }

    /// Resume processes in a session.
    pub fn resume(&self) -> Result<()> {
        self.signal("SIGCONT")
    }

    /// Pause processes in a session.
    pub fn pause(&self) -> Result<()> {
        self.signal("SIGSTOP")
    }
}
// handler:1 ends here

// [[file:../vasp-tools.note::*shared child][shared child:1]]
mod shared {
    use super::*;
    use shared_child::unix::SharedChildExt;
    use shared_child::SharedChild;

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

            Ok(())
        }
    }
}
// shared child:1 ends here

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Session {
    fn drop(&mut self) {
        if let Err(e) = self.quit() {
            error!("drop session error: {:?}", e);
        }
    }
}
// drop:1 ends here
