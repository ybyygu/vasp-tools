// [[file:../vasp-tools.note::*docs][docs:1]]
//! Run child processes in a new session group for easy to interact and control.
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*core/rexpect][core/rexpect:1]]
use rexpect::session::PtySession;
use std::process::{ChildStdin, ChildStdout};

/// Run child processes in a new session group for easy control
pub struct Session {
    command: Option<Command>,
    session: Option<PtySession>,
}

impl Session {
    /// Create a new session for running `command`
    pub fn new(command: Command) -> Self {
        Self {
            command: command.into(),
            session: None,
        }
    }

    /// Return child process's session ID, useful for killing all child
    /// processes using `pkill` command.
    pub fn id(&self) -> Option<u32> {
        let sid = self.session.as_ref()?.process.child_pid.as_raw();

        Some(sid as u32)
    }

    pub(crate) fn spawn_new(&mut self) -> Result<()> {
        let command = self.command.take().unwrap();
        self.session = create_new_session(command)?.into();
        info!("start child process in new session: {:?}", self.id());
        Ok(())
    }

    /// Interact with child process's stdin using `input` and return stdout
    /// read-in until the line matching `read_pattern`. The child process will
    /// be automatically spawned if necessary.
    pub fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
        use std::io::prelude::*;

        // create a new session for the first time
        if self.session.is_none() {
            self.spawn_new()?;
        }
        let s = self.session.as_mut().expect("rexpect session");

        // ignore interaction with empty input
        if !input.is_empty() {
            trace!("send input for child process's stdin ({} bytes)", input.len());
            s.send_line(input)
                .map_err(|e| format_err!("send input error: {:?}", e))?;
        }

        trace!("send read pattern for child process's stdout: {:?}", read_pattern);
        let mut txt = String::new();
        while let Ok(line) = s.read_line() {
            writeln!(&mut txt, "{}", line)?;
            if line.contains(read_pattern) {
                break;
            }
        }

        // NOTE: rexpect's reading behavior is weild
        // let (txt, _) = s
        //     .exp_any(vec![
        //         rexpect::ReadUntil::String(read_pattern.into()),
        //         rexpect::ReadUntil::EOF,
        //     ])
        //     // .exp_regex(read_pattern)
        //     .map_err(|e| format_err!("read stdout error: {:?}", e))?;
        // To make parsing results easier, we remove all `\r` chars, which is added by rexecpt for each line
        // return Ok(txt.replace("\r", ""));

        if txt.is_empty() {
            bail!("Got nothing for pattern: {}", read_pattern);
        }
        return Ok(txt);
    }
}

/// Spawn child process in a new session
fn create_new_session(command: Command) -> Result<PtySession> {
    use rexpect::session::spawn_command;

    let session = spawn_command(command, None).map_err(|e| format_err!("spawn command error: {:?}", e))?;

    Ok(session)
}

#[test]
fn test_session_interact() -> Result<()> {
    gut::cli::setup_logger_for_test();

    let sh = std::process::Command::new("tests/files/interactive-job.sh");
    let mut s = Session::new(sh);

    let o = s.interact("test1\n", "POSITIONS: reading from stdin")?;
    assert!(o.contains("mag=     2.2094"));
    let o = s.interact("test1\n", "POSITIONS: reading from stdin")?;
    assert!(o.contains("mag=     2.3094"));

    Ok(())
}
// core/rexpect:1 ends here

// [[file:../vasp-tools.note::*signal][signal:1]]
impl Session {
    /// send signal to child processes
    ///
    /// SIGINT, SIGTERM, SIGCONT, SIGSTOP
    fn signal(&self, sig: &str) -> Result<()> {
        if let Some(sid) = self.id() {
            signal_processes_by_session_id(sid, sig)?;
        } else {
            bail!("session not started yet");
        }
        Ok(())
    }

    /// Terminate child processes in a session.
    pub fn terminate(&self) -> Result<()> {
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

/// Call `pkill` to send signal to related processes
fn signal_processes_by_session_id(sid: u32, signal: &str) -> Result<()> {
    duct::cmd!("pkill", "-s", sid.to_string()).unchecked().run()?;

    Ok(())
}

/// signal processes by session id
fn signal_processes_by_session_id_alt(sid: u32, signal: &str) -> Result<()> {
    // cmdline: kill -CONT -- $(ps -s $1 -o pid=)
    let output = duct::cmd!("ps", "-s", format!("{}", sid), "-o", "pid=").read()?;
    let pids: Vec<_> = output.split_whitespace().collect();

    let mut args = vec!["-s", signal, "--"];
    args.extend(&pids);
    if !pids.is_empty() {
        duct::cmd("kill", &args).unchecked().run()?;
    } else {
        info!("No remaining processes found!");
    }

    Ok(())
}
// signal:1 ends here

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Session {
    fn drop(&mut self) {
        if let Some((sid, status)) = self.id().zip(self.status()) {
            dbg!(sid, status);
            let _ = self.terminate();
        }
    }
}

impl Session {
    fn status(&self) -> Option<rexpect::process::wait::WaitStatus> {
        let status = self.session.as_ref()?.process.status()?;
        status.into()
    }
}
// drop:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use std::process::Command;

pub fn new_session(program: &std::path::Path) -> Session {
    let command = Command::new(program);
    Session::new(command)
}
// pub:1 ends here

// [[file:../vasp-tools.note::*test][test:1]]
#[test]
fn test_interactive_vasp() -> Result<()> {
    let read_pattern = "POSITIONS: reading from stdin";

    // the input for writing into stdin
    let positions = include_str!("../tests/files/interactive_positions.txt");

    let vasp = std::process::Command::new("fake-vasp");
    let mut s = Session::new(vasp);

    let o = s.interact("", read_pattern)?;
    let (energy1, forces1) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    // println!("{}", o);
    let o = s.interact(&positions, read_pattern)?;
    let (energy2, forces2) = crate::vasp::stdout::parse_energy_and_forces(&o)?;
    assert_eq!(energy1, energy2);
    // println!("{}", o);

    s.terminate()?;

    Ok(())
}
// test:1 ends here
