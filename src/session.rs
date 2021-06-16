// [[file:../vasp-tools.note::*docs][docs:1]]
//! Run child processes in a new session group for easy to interact and control.
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;

use gosh::runner::prelude::*; // new_process_group, spawn_session
// imports:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
/// Call `pkill` to send signal to related processes
fn signal_processes_by_session_id(sid: u32, signal: &str) -> Result<()> {
    trace!("Kill session {} using signal {:?}", sid, signal);
    duct::cmd!("pkill", "--signal", signal, "-s", sid.to_string())
        .unchecked()
        .run()?;

    Ok(())
}
// base:1 ends here

// [[file:../vasp-tools.note::*core/std][core/std:1]]
mod core_std {
    use super::*;
    use std::process::Command;
    use std::process::{Child, ExitStatus};

    /// Run child processes in a new session for easy control
    pub struct Session {
        command: Option<Command>,
        stream0: Option<stdin::StdinWriter>,
        stream1: Option<stdout::StdoutReader>,
        session_handler: Option<SessionHandler>,
    }

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
                stream0: None,
                stream1: None,
                session_handler: None,
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
            // ignore interaction with empty input
            let stdin = self.stream0.as_mut().unwrap();
            if !input.is_empty() {
                trace!("send input for child process's stdin ({} bytes)", input.len());
                stdin.write(input)?;
            }

            trace!("send read pattern for child process's stdout: {:?}", read_pattern);
            let stdout = self.stream1.as_mut().unwrap();
            let txt = stdout.read_until(read_pattern)?;
            if txt.is_empty() {
                bail!("Got nothing for pattern: {}", read_pattern);
            }
            return Ok(txt);
        }

        /// Return child process's session ID.
        pub fn id(&self) -> Option<u32> {
            self.session_handler.as_ref().map(|s| s.id())
        }

        /// Spawn child process in new session (progress group), and return a
        /// `SessionHandler` that can be shared between threads.
        pub fn spawn(&mut self) -> Result<SessionHandler> {
            let command = self.command.take().unwrap();
            let mut child = create_new_session(command)?;
            self.stream0 = stdin::StdinWriter::new(child.stdin.take().unwrap()).into();
            self.stream1 = stdout::StdoutReader::new(child.stdout.take().unwrap()).into();
            let h = SessionHandler::new(child);
            self.session_handler = h.clone().into();
            let pid = self.id().unwrap();
            info!("start child process in new session: {:?}", pid);

            Ok(h)
        }

        /// Create a session handler for shared between threads.
        pub fn get_handler(&self) -> Option<SessionHandler> {
            self.session_handler.clone()
        }
    }
}
// core/std:1 ends here

// [[file:../vasp-tools.note::*stdin][stdin:1]]
mod stdin {
    use super::*;
    use std::io::Write;
    use std::process::ChildStdin;

    pub struct StdinWriter {
        stdin: ChildStdin,
    }

    impl StdinWriter {
        pub fn new(stdin: ChildStdin) -> Self {
            Self { stdin }
        }

        /// Write `input` into self's stdin
        pub fn write(&mut self, input: &str) -> Result<()> {
            self.stdin.write_all(input.as_bytes())?;
            self.stdin.flush()?;
            trace!("wrote stdin done: {} bytes", input.len());

            Ok(())
        }
    }
}
// stdin:1 ends here

// [[file:../vasp-tools.note::*stdout][stdout:1]]
mod stdout {
    use super::*;

    use gut::prelude::*;
    use std::io::{self, BufRead, Write};
    use std::process::ChildStdout;

    pub struct StdoutReader {
        reader: io::Lines<io::BufReader<ChildStdout>>,
    }

    impl StdoutReader {
        pub fn new(stdout: ChildStdout) -> Self {
            let reader = io::BufReader::new(stdout).lines();
            Self { reader }
        }

        /// Read stdout until finding a line containing the `pattern`
        pub fn read_until(&mut self, pattern: &str) -> Result<String> {
            info!("Read stdout until finding pattern: {:?}", pattern);
            let mut text = String::new();
            while let Some(line) = self.reader.next() {
                let line = line.context("invalid encoding?")?;
                writeln!(&mut text, "{}", line)?;
                if line.contains(&pattern) {
                    info!("found pattern: {:?}", pattern);
                    return Ok(text);
                }
            }
            bail!("Expected pattern not found: {:?}!", pattern);
        }
    }
}
// stdout:1 ends here

// [[file:../vasp-tools.note::*session handler][session handler:1]]
mod handler_std {
    use super::*;
    use shared_child::SharedChild;
    use std::process::{Child, Command, ExitStatus};

    /// A simple wrapper around `shared_child::SharedChild`, which is usually
    /// created by `Session::spawn` method.
    ///
    /// Control progress group in session using external `pkill` command.
    #[derive(Debug, Clone)]
    pub struct SessionHandler {
        inner: std::sync::Arc<SharedChild>,
    }

    impl SessionHandler {
        /// Create a `SessionHandler` from std::process::Child
        pub(super) fn new(s: Child) -> Self {
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
        fn try_wait(&self) -> Result<Option<ExitStatus>> {
            let o = self.inner.try_wait()?;
            Ok(o)
        }

        /// Wait for the child to exit, blocking the current thread, and return its
        /// exit status.
        fn wait(&self) -> Result<ExitStatus> {
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
        fn kill(&self) -> Result<()> {
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
}
// session handler:1 ends here

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(s) = self.get_handler() {
            if let Err(e) = s.terminate() {
                error!("drop session error: {:?}", e);
            }
        }
    }
}
// drop:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use core_std::Session;
pub use handler_std::SessionHandler;
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
    let _ = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    let o = s.interact(&positions, read_pattern)?;
    let (energy2, _forces2) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    assert_eq!(energy2, 2.0);
    let o = s.interact(&positions, read_pattern)?;
    let (energy3, _forces3) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    assert_eq!(energy3, 3.0);

    h.terminate()?;

    Ok(())
}
// test:1 ends here
