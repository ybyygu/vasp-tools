// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use std::path::Path;
// imports:1 ends here

// [[file:../vasp-server.note::*base][base:1]]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use std::io::prelude::*;
use std::io::BufReader;
use std::io::LineWriter;

struct Task {
    child: Child,
    stdout: Option<ChildStdout>,
    stdin: Option<ChildStdin>,
}

impl Task {
    fn new<P: AsRef<Path>>(exe: P) -> Result<Self> {
        let mut child = Command::new(exe.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take();
        let stdin = child.stdin.take();

        Ok(Self { child, stdin, stdout })
    }

    fn stdout(&mut self) -> BufReader<ChildStdout> {
        let r = self.stdout.take().unwrap();
        BufReader::new(r)
    }

    fn stdin(&mut self) -> LineWriter<ChildStdin> {
        let r = self.stdin.take().unwrap();
        LineWriter::new(r)
    }
}
// base:1 ends here

// [[file:../vasp-server.note::*core][core:1]]
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

fn read_interactive_vasp_output(stdout: &mut ChildStdout, natoms: usize) -> Result<()> {
    todo!()
}
// core:1 ends here

// [[file:../vasp-server.note::*test][test:1]]
enum ReadState {
    Skip,
    Forces,
    Energy,
}

#[derive(Debug, Default, Clone)]
struct VaspResult {
    forces: String,
    energy: String,
}

impl VaspResult {
    fn collect(&mut self, line: &str, state: &ReadState) {
        use ReadState::*;

        match state {
            Forces => {
                self.forces.push_str(line);
                self.forces.push_str("\n");
            }
            Energy => {
                self.energy.push_str(line);
            }
            _ => {}
        }
    }

    fn get_energy(&self) -> Option<f64> {
        parse_vasp_energy(&self.energy)
    }

    fn get_forces(&self) -> Option<Vec<[f64; 3]>> {
        parse_vasp_forces(&self.forces)
    }
}

fn parse_vasp_forces(s: &str) -> Option<Vec<[f64; 3]>> {
    if s.is_empty() {
        None
    } else {
        s.lines()
            .skip(1)
            .map(|line| {
                let parts: Vec<_> = line.split_whitespace().map(|p| p.parse().unwrap()).collect();
                [parts[0], parts[1], parts[2]]
            })
            .collect_vec()
            .into()
    }
}

fn parse_vasp_energy(s: &str) -> Option<f64> {
    if s.len() < 42 {
        None
    } else {
        s[26..26 + 16].trim().parse().ok()
    }
}

#[test]
fn test_parse_vasp_energy() {
    let s = "   1 F= -.84780990E+02 E0= -.84775142E+02  d E =-.847810E+02  mag=     3.2666";
    let e = parse_vasp_energy(s);
    assert_eq!(e, Some(-0.84775142E+02));
}

#[test]
fn test_run() -> Result<()> {
    let mut task = Task::new("/tmp/test.sh")?;
    let stdout = task.stdout();

    let mut lines = stdout.lines();
    let mut results = VaspResult::default();
    let mut state = ReadState::Skip;
    loop {
        if let Some(line) = lines.next() {
            let line = line?;
            if line == "FORCES:" {
                info!("start reading forces");
                state = ReadState::Forces;
            } else if line.starts_with("1 F=") {
                state = ReadState::Energy;
                info!("start reading energy");
            } else if line == "POSITIONS: reading from stdin" {
                info!("read input structure from stdin");
                break;
            }
            results.collect(&line, &state);
        } else {
            break;
        }
    }
    let mut stdin = task.stdin();
    stdin.write_all(b"exit\n");
    dbg!(results);

    for line in lines {
        let line = line?;
        dbg!(line);
    }

    Ok(())
}
// test:1 ends here
