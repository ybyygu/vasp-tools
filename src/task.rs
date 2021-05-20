// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;

use gchemol::prelude::*;
use gchemol::Molecule;
use gosh::gchemol;

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use std::io::prelude::*;
use std::io::BufReader;
use std::io::LineWriter;

pub(crate) struct Task {
    child: Child,
    stream0: ChildStdin,
    stream1: std::io::Lines<BufReader<ChildStdout>>,
}

impl Task {
    pub fn new(mut child: Child) -> Self {
        let stream0 = child.stdin.take().unwrap();
        let stream1 = child.stdout.take().unwrap();
        Self {
            stream0,
            stream1: BufReader::new(stream1).lines(),
            child,
        }
    }
}
// base:1 ends here

// [[file:../vasp-tools.note::*stop][stop:1]]
impl Drop for Task {
    fn drop(&mut self) {
        // if let Err(msg) = crate::vasp::write_stopcar() {
        //     eprintln!("Failed to stop vasp server: {:?}", msg);
        // }
    }
}
// stop:1 ends here

// [[file:../vasp-tools.note::*compute & output][compute & output:1]]
use gosh::model::ModelProperties;

impl Task {
    /// write scaled positions to VASP stdin
    fn input_positions(&mut self, mol: &Molecule) -> Result<()> {
        info!("write scaled positions into stdin");
        let mut lines = mol
            .get_scaled_positions()
            .expect("lattice")
            .map(|[x, y, z]| format!("{:19.16} {:19.16} {:19.16}\n", x, y, z));

        for line in lines {
            self.stream0.write_all(line.as_bytes())?;
        }
        self.stream0.flush()?;

        Ok(())
    }

    fn compute_mol(&mut self, mol: &Molecule) -> Result<ModelProperties> {
        let mut text = String::new();
        while let Some(line) = self.stream1.next() {
            let line = line?;
            if line.starts_with("POSITIONS: reading from stdin") {
                let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&text)?;
                let mut mp = ModelProperties::default();
                mp.set_energy(energy);
                mp.set_forces(forces);
                return Ok(mp);
            }
            writeln!(&mut text, "{}", line)?;
        }
        bail!("no model properties found!");
    }

    /// Caclculate model properties in an interactive fashion (with child
    /// process)
    ///
    /// # Parameters
    ///
    /// * mol: the molecule to be calculated
    /// * n: the total number of computations
    pub fn interact(&mut self, mol: &Molecule, n: usize) -> Result<ModelProperties> {
        info!("interact with vasp process ...");
        if n != 0 {
            info!("input positions");
            self.input_positions(mol)?;
        }
        info!("recv outputs ...");
        let mp = self.compute_mol(mol)?;

        Ok(mp)
    }
}
// compute & output:1 ends here

// [[file:../vasp-tools.note::*core][core:1]]

// core:1 ends here
