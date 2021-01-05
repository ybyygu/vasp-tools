// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use std::os::unix::net::UnixStream;
use std::path::Path;
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
    child: Child,
    // stdout: Option<ChildStdout>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl Task {
    pub(crate) fn new<P: AsRef<Path>>(exe: P) -> Result<Self> {
        let exe = exe.as_ref();
        let mut child = Command::new(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("run script: {:?}", exe))?;

        let stdout = child.stdout.take().and_then(|x| BufReader::new(x).into());
        let stdin = child.stdin.take();

        Ok(Self { child, stdin, stdout })
    }
}
// base:1 ends here

// [[file:../vasp-server.note::*core][core:1]]
impl Task {
    // 从POSCAR中提取分数坐标, 写入vasp stdin
    fn take_action_input(&mut self) -> Result<()> {
        use gchemol::prelude::*;
        use gchemol::Molecule;
        use gosh::gchemol;

        // FIXME: read POSITIONS from POSCAR
        debug!("Read scaled positions from POSCAR file ...");
        let mol = Molecule::from_file("POSCAR").context("Reading POSCAR file ...")?;
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
// core:1 ends here

// [[file:../vasp-server.note::*vasp][vasp:1]]
mod vasp {
    use super::*;
    use text_parser::parsers::*;

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
        let (_, e) = read_energy(s).unwrap();
        assert_eq!(e, -0.84775142E+02);
    }

    // FORCES:
    //      0.2084558     0.2221942    -0.1762308
    //     -0.1742340     0.2172782     0.2304866
    //      0.2244132    -0.1794341     0.2106465
    //     -0.2907316    -0.2746548    -0.2782190
    //     -0.2941880    -0.0306001    -0.0141722
    fn read_forces(s: &str) -> IResult<&str, Vec<[f64; 3]>> {
        let tag_forces = tag("FORCES:");
        let read_forces = many1(read_xyz);

        do_parse!(
            s,
            tag_forces >> eol   >>     // FORCES:
            forces: read_forces >>     // forces in each line
            (forces)
        )
    }

    //      0.2084558     0.2221942    -0.1762308
    fn read_xyz(s: &str) -> IResult<&str, [f64; 3]> {
        do_parse!(
            s,
            space1 >> xyz: xyz_array >> read_line >> // ignore the remaining characters
            (xyz)
        )
    }

    //    1 F= -.85097948E+02 E0= -.85096866E+02  d E =-.850979E+02  mag=     2.9646
    //    2 F= -.85086257E+02 E0= -.85082618E+02  d E =-.850863E+02  mag=     2.9772
    // POSITIONS: reading from stdin
    fn read_energy(s: &str) -> IResult<&str, f64> {
        let tag_nf = tag("F=");
        let tag_e0 = tag("E0=");
        do_parse!(
            s,
            space0 >> digit1 >> space1 >> tag_nf >> space0 >> double >>  // 1 F= ...
            space0 >> tag_e0 >> space0 >> energy: double >> read_line >> // E0= ...
            (energy)
        )
    }

    fn read_energy_and_forces(s: &str) -> IResult<&str, (f64, Vec<[f64; 3]>)> {
        let jump = take_until("FORCES:\n");
        do_parse!(
            s,
            jump >>                 // skip leading text until found "FORCES"
            forces: read_forces >>  // read forces
            energy: read_energy >>  // read forces
            ((energy, forces))
        )
    }

    pub(super) fn parse_energy_and_forces(s: &str) -> Result<(f64, Vec<[f64; 3]>)> {
        let (_, values) = read_energy_and_forces(s).unwrap();
        Ok(values)
    }

    #[test]
    fn test_parse_vasp_interactive() -> Result<()> {
        let s = "./tests/files/interactive.txt";
        let s = gut::fs::read_file(s)?;

        let (e, f) = parse_energy_and_forces(&s)?;
        assert_eq!(f.len(), 25);

        Ok(())
    }
}
// vasp:1 ends here

// [[file:../vasp-server.note::*compute][compute:1]]
use gosh::gchemol::Molecule;
use gosh::model::ModelProperties;

impl Task {
    pub fn compute_mol(&mut self, mol: &Molecule) -> Result<ModelProperties> {
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
                    let (energy, forces) = self::vasp::parse_energy_and_forces(&text)?;
                    let mut mp = ModelProperties::default();
                    mp.set_energy(energy);
                    mp.set_forces(forces);
                    return Ok(mp);
                }
            }
        }
    }
}
// compute:1 ends here
