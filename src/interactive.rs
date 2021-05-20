// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;

use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*task][task:1]]
use rexpect::reader::{NBReader, ReadUntil};

/// Struct for interactive communication with child process's standard input and
/// standard output, simultaneously without deadlock.
pub struct Task {
    child: std::process::Child,
    stream0: std::process::ChildStdin,
    stream1: NBReader,
}

impl Task {
    /// Create `Task` from child process.
    ///
    /// # Panics
    ///
    /// * Will panic if stdin/stdout of child process not captured
    pub fn new(mut child: std::process::Child) -> Self {
        let stream0 = child.stdin.take().expect("no piped stdin");
        let stream1 = child.stdout.take().expect("no piped stdout");
        Self {
            stream0,
            stream1: NBReader::new(stream1, None),
            child,
        }
    }

    /// feed child process's standard input with `input` stream
    pub fn write_stdin(&mut self, input: &str) -> Result<()> {
        use std::io::Write;

        self.stream0.write_all(input.as_bytes())?;
        self.stream0.flush()?;

        Ok(())
    }

    /// read output from process stdout until matching a `pattern`
    pub fn read_output_until(&mut self, pattern: &str) -> Result<String> {
        let (txt, _) = self
            .stream1
            .read_until(&ReadUntil::String(pattern.into()))
            .expect("read POSITIONS");

        Ok(txt)
    }
}
// task:1 ends here

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Task {
    fn drop(&mut self) {
        dbg!();
    }
}
// drop:1 ends here

// [[file:../vasp-tools.note::*test][test:1]]
#[test]
fn test_task() {
    use std::process::{Command, Stdio};

    let child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut task = Task::new(child);
    task.write_stdin("test1\n").unwrap();
    let x = task.read_output_until("\n").unwrap();
    assert_eq!(x, "test1");
    task.write_stdin("test2\n").unwrap();
    let x = task.read_output_until("\n").unwrap();
    assert_eq!(x, "test2");
}
// test:1 ends here
