// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use gchemol::prelude::*;
use gchemol::Molecule;
use gosh::gchemol;

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-server.note::*constants][constants:1]]
const SOCKET_FILE: &str = "VASP.socket";
// constants:1 ends here

// [[file:../vasp-server.note::*base][base:1]]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use std::io::prelude::*;
use std::io::BufReader;
use std::io::LineWriter;

#[derive(Debug)]
pub(crate) struct Task {
    exe: PathBuf,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl Task {
    pub fn new<P: AsRef<Path>>(exe: P) -> Self {
        let exe = exe.as_ref().to_owned();

        Self {
            exe,
            child: None,
            stdin: None,
            stdout: None,
        }
    }

    fn start_cmd(&mut self) -> Result<()> {
        let mut child = Command::new(&self.exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("run script: {:?}", &self.exe))?;

        let stdout = child.stdout.take().and_then(|x| BufReader::new(x).into());
        let stdin = child.stdin.take();

        self.stdin = stdin;
        self.stdout = stdout;

        Ok(())
    }

    fn is_started(&self) -> bool {
        self.child.is_some()
    }
}
// base:1 ends here

// [[file:../vasp-server.note::*stop][stop:1]]
impl Drop for Task {
    fn drop(&mut self) {
        if let Err(msg) = crate::vasp::stop_vasp_server() {
            eprintln!("Failed to stop vasp server: {:?}", msg);
        }
    }
}
// stop:1 ends here

// [[file:../vasp-server.note::*input][input:1]]
impl Task {
    /// write scaled positions to VASP stdin
    fn input_positions(&mut self, mol: &Molecule) -> Result<()> {
        let lines: String = mol
            .get_scaled_positions()
            .expect("lattice")
            .map(|[x, y, z]| format!("{:19.16}{:19.16}{:19.16}\n", x, y, z))
            .collect();

        let mut writer = std::io::BufWriter::new(self.stdin.as_mut().unwrap());
        writer.write_all(lines.as_bytes())?;
        writer.flush()?;

        Ok(())
    }
}
// input:1 ends here

// [[file:../vasp-server.note::*compute & output][compute & output:1]]
use gosh::model::ModelProperties;

impl Task {
    pub fn compute_mol(&mut self, mol: &Molecule) -> Result<ModelProperties> {
        if !self.is_started() {
            mol.to_file("POSCAR").context("write POSCAR for initial calculation")?;
            self.start_cmd()?;
        } else {
            self.input_positions(mol)?;
        }

        let stdout = self.stdout.as_mut().unwrap();
        let mut text = String::new();
        let mut lines = stdout.lines();
        loop {
            // if let Some(exit_code) = self.child.try_wait().context("wait child process")? {
            //     info!("child process exited with code {}", exit_code);
            //     break;
            // }
            if let Some(line) = lines.next() {
                let line = line?;
                writeln!(&mut text, "{}", line)?;
                if line == "POSITIONS: reading from stdin" {
                    let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&text)?;
                    let mut mp = ModelProperties::default();
                    mp.set_energy(energy);
                    mp.set_forces(forces);
                    return Ok(mp);
                }
            }
        }
    }
}
// compute & output:1 ends here
