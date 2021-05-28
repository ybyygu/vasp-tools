// [[file:../vasp-tools.note::*docs][docs:1]]
//! This mod is for VASP interactive calculations.
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;

use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
use crate::session::{Command, Session};
use crate::process::PidFile;

/// Struct for interactive communication with child process's standard input and
/// standard output, simultaneously without deadlock.
pub struct Task {
    session: Session,
    pidfile: Option<PidFile>,
}

impl Task {
    /// Create `Task` for long live interactive calculation.
    pub fn new(command: Command) -> Self {
        let session = Session::new(command);
        Self { pidfile: None, session }
    }

    /// feed child process's standard input with `input` stream, and read output
    /// from process stdout until matching `read_pattern`
    pub fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
        let out = self.session.interact(input, read_pattern)?;
        // create PIDFILE when child process started
        if let Some(sid) = self.session.id() {
            let pidfile = PidFile::new("vasp.pid".as_ref(), sid)?;
            self.pidfile = Some(pidfile);
        }

        Ok(out)
    }

    /// Pause task
    pub fn pause(&self) -> Result<()> {
        self.session.pause()?;
        Ok(())
    }
    
    /// Resume paused task
    pub fn resume(&self) -> Result<()> {
        self.session.resume()?;
        Ok(())
    }
    /// Force to terminate running task
    pub fn quit(&self) -> Result<()> {
        self.session.terminate()?;
        Ok(())
    }
}
// base:1 ends here

// [[file:../vasp-tools.note::*shared][shared:1]]
mod shared {
    use super::*;
    use crate::session::{Command, Session};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    pub struct SharedTask {
        pidfile: Option<PidFile>,
        task: Arc<Mutex<Session>>,
    }

    impl SharedTask {
        /// Feed child process's standard input with `input` stream, and read
        /// output from process stdout until matching `read_pattern`. Call this
        /// function will block for locking child process.
        pub async fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
            info!("lock task");
            let mut task = self.task.lock().unwrap();
            info!("lock task done");
            let txt = task.interact(input, read_pattern)?;
            info!("interact done");
            // create PIDFILE when ready
            if self.pidfile.is_none() {
                if let Some(sid) = task.id() {
                    info!("create pidfile for {}", sid);
                    self.pidfile = PidFile::new("vasp.pid".as_ref(), sid)?.into();
                }
            }
            Ok(txt)
        }

        fn pidfile(&self) -> Result<&PidFile> {
            let pidfile = self.pidfile.as_ref().ok_or(format_err!("no active process"))?;
            Ok(pidfile)
        }

        /// Pause task
        pub async fn pause(&self) -> Result<()> {
            self.pidfile()?.pause()?;
            Ok(())
        }

        /// Resume paused task
        pub async fn resume(&self) -> Result<()> {
            self.pidfile()?.resume()?;
            Ok(())
        }

        /// Force to terminate running task
        pub async fn terminate(&self) -> Result<()> {
            self.pidfile()?.terminate()?;
            Ok(())
        }
    }

    pub fn new_shared_task(command: Command) -> SharedTask {
        let task = Session::new(command);

        let shared = SharedTask {
            task: Arc::new(Mutex::new(task)),
            pidfile: None,
        };

        shared
    }

    #[derive(Debug, Clone)]
    struct PidFile {
        pid: u32,
        path: PathBuf,
    }

    impl PidFile {
        /// Create a pidfile for process `pid`
        pub fn new(path: &Path, pid: u32) -> Result<Self> {
            gut::fs::write_to_file(path, &format!("{}", pid))?;

            let pidfile = Self {
                path: path.to_owned(),
                pid,
            };

            Ok(pidfile)
        }
    }

    impl PidFile {
        /// send signal to child processes
        ///
        /// SIGINT, SIGTERM, SIGCONT, SIGSTOP
        fn signal(&self, sig: &str) -> Result<()> {
            info!("signal process {} with {}", self.pid, sig);
            signal_processes_by_session_id(self.pid, sig)?;
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

    impl Drop for PidFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Call `pkill` to send signal to related processes
    fn signal_processes_by_session_id(sid: u32, signal: &str) -> Result<()> {
        duct::cmd!("pkill", "-s", sid.to_string()).unchecked().run()?;

        Ok(())
    }
}
// shared:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use shared::*;
// pub:1 ends here
