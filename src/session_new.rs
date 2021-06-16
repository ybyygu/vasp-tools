// [[file:../vasp-tools.note::*docs][docs:1]]
//! Run child processes in a new session group for easy to interact and control.
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;

use gosh::runner::prelude::*; // new_process_group, spawn_session
// imports:1 ends here

// [[file:../vasp-tools.note::*core/std][core/std:1]]
mod core_std {
    use super::*;
    use gosh::runner::SessionHandler;
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
            self.session_handler.as_ref().and_then(|s| s.id())
        }

        /// Spawn child process in new session (progress group), and return a
        /// `SessionHandler` that can be shared between threads.
        pub fn spawn(&mut self) -> Result<SessionHandler> {
            use std::process::Stdio;

            // we want to interact with child process's stdin and stdout
            let mut command = self.command.take().unwrap();
            let mut session = command.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn_session()?;
            self.stream0 = stdin::StdinWriter::new(session.child.stdin.take().unwrap()).into();
            self.stream1 = stdout::StdoutReader::new(session.child.stdout.take().unwrap()).into();

            let h = session.handler();
            self.session_handler = h.clone().into();
            info!("start child process in new session: {:?}", h.id());

            Ok(h.clone())
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
// FIXME: rewrite
pub use gosh::runner::SessionHandler;
// pub:1 ends here
