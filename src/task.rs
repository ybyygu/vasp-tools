// [[file:../vasp-server.note::*task.rs][task.rs:1]]
use gut::prelude::*;

use std::io::prelude::*;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Send some input to a child process.
fn send_input(child: &mut Child, input: &str) -> Result<()> {
    child
        .stdin
        .as_mut()
        .expect("cmd stdin")
        .write_all(input.as_bytes())
        .context("write child process stdin")?;

    Ok(())
}

/// Read stdout of a child process
fn read_output(child: &mut Child) -> Result<String> {
    let mut out = String::new();
    child
        .stdout
        .as_mut()
        .expect("cmd stdout")
        .read_to_string(&mut out)
        .context("write child process stdin")?;

    Ok(out)
}

fn start_cmd(script_file: &Path) -> Result<Child> {
    let child = Command::new(script_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    Ok(child)
}

use std::os::unix::net::UnixStream;

fn redirect_cmd_stdout(child: &mut Child, stream: &mut UnixStream) -> Result<()> {
    let stdout = child.stdout.as_mut().expect("cmd stdout");

    std::io::copy(stdout, stream)?;

    Ok(())
}

fn redirect_cmd_stdin(child: &mut Child, stream: &mut UnixStream) -> Result<()> {
    let stdin = child.stdin.as_mut().expect("cmd stdin");

    std::io::copy(stream, stdin)?;

    Ok(())
}
// task.rs:1 ends here
