// [[file:../vasp-tools.note::*docs][docs:1]]
//! Run child processes in a new session group for easy to interact and control.
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
use crate::process::ProcessGroupExt;

use shared_child::SharedChild;
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::process::{Child, ChildStdin, ChildStdout, ExitStatus};
// imports:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
/// Run child processes in a new session for easy control
pub struct Session {
    command: Option<Command>,
    session: Option<SessionHandler>,
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

    /// Interact with child process's stdin using `input` and return stdout
    /// read-in until the line matching `read_pattern`. The `spawn` method
    /// should be called before `interact`.
    ///
    /// # Panics
    ///
    /// * panic if child process is not spawned yet.
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

    /// Return child process's session ID.
    pub fn id(&self) -> Option<u32> {
        self.session.as_ref().map(|s| s.id())
    }

    /// Spawn child process in new session (progress group), and return a
    /// `SessionHandler` that can be shared between threads.
    pub fn spawn(&mut self) -> Result<SessionHandler> {
        let command = self.command.take().unwrap();
        let mut child = create_new_session(command)?;
        self.stream0 = child.stdin.take().unwrap().into();
        let stdout = child.stdout.take().unwrap();
        self.stream1 = BufReader::new(stdout).lines().into();
        let h = SessionHandler::new(child);
        self.session = h.clone().into();
        let pid = self.id().unwrap();
        info!("start child process in new session: {:?}", pid);

        Ok(h)
    }

    /// Create a session handler for shared between threads.
    pub fn get_handler(&self) -> Option<SessionHandler> {
        self.session.clone()
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
/// A simple wrapper around `shared_child::SharedChild`
///
/// Control progress group in session using external `pkill` command.
#[derive(Debug, Clone)]
pub struct SessionHandler {
    inner: std::sync::Arc<SharedChild>,
}

impl SessionHandler {
    /// Create a `SessionHandler` from std::process::Child
    pub fn new(s: Child) -> Self {
        Self {
            inner: std::sync::Arc::new(SharedChild::new(s)),
        }
    }
}

impl SessionHandler {
    /// send signal to child processes: SIGINT, SIGTERM, SIGCONT, SIGSTOP
    fn signal(&self, sig: &str) -> Result<()> {
        // only using pkill when child process is still running
        match self.try_wait() {
            Ok(None) => {
                let pid = self.id();
                info!("signal process {} with {}", pid, sig);
                signal_processes_by_session_id(pid, sig)?;
            }
            Ok(Some(n)) => {
                info!("child process already exited with code: {}", n);
            }
            Err(e) => {
                error!("failed to check child process'status: {:?}", e);
            }
        }
        Ok(())
    }

    /// Return the child process ID.
    pub fn id(&self) -> u32 {
        self.inner.id()
    }

    /// Return the child’s exit status if it has already exited. If the child is
    /// still running, return Ok(None).
    pub fn try_wait(&self) -> Result<Option<ExitStatus>> {
        let o = self.inner.try_wait()?;
        Ok(o)
    }

    /// Wait for the child to exit, blocking the current thread, and return its
    /// exit status.
    pub fn wait(&self) -> Result<ExitStatus> {
        let o = self.inner.wait()?;
        info!("child process exited with code: {:?}", o);
        Ok(o)
    }

    /// Terminate child processes in a session.
    pub fn terminate(&self) -> Result<()> {
        // If process was paused, terminate it directly could result a deadlock or zombie.
        self.signal("SIGCONT")?;
        sleep(0.2);
        self.signal("SIGTERM")?;
        self.wait()?;
        // according to the doc of `SharedChild`, we should wait for it to exit.
        Ok(())
    }

    /// Kill processes in a session.
    pub fn kill(&self) -> Result<()> {
        self.signal("SIGCONT")?;
        sleep(0.2);
        self.signal("SIGKILL")?;
        // according to the doc of `SharedChild`, we should wait for it to exit.
        self.wait()?;
        Ok(())
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

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(s) = self.session.as_ref() {
            if let Err(e) = s.terminate() {
                error!("drop session error: {:?}", e);
            }
        }
    }
}
// drop:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]

// pub:1 ends here

// [[file:../vasp-tools.note::*test][test:1]]
#[test]
fn test_interactive_vasp() -> Result<()> {
    let read_pattern = "POSITIONS: reading from stdin";

    // the input for writing into stdin
    let positions = include_str!("../tests/files/interactive_positions.txt");

    let vasp = std::process::Command::new("fake-vasp");
    let mut s = Session::new(vasp);
    let h = s.spawn()?;

    let o = s.interact("", read_pattern)?;
    let (energy1, forces1) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    println!("{}", o);
    let o = s.interact(&positions, read_pattern)?;
    let (energy2, forces2) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    assert_eq!(energy1, energy2);
    
    h.terminate()?;

    Ok(())
}
// test:1 ends here
