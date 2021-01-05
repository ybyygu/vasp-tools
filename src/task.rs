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

#[derive(Debug, PartialEq, Eq, Clone)]
enum ReadState {
    // 可忽略
    Skip,
    // 当前结构forces行
    Forces,
    // 当前结构能量行
    Energy,
    // 需要在stdin写入新结构的分数坐标
    InputPositions,
}

#[derive(Debug)]
pub(crate) struct Task {
    child: Child,
    stdout: Option<ChildStdout>,
    stdin: Option<ChildStdin>,
    stdout_reader: Option<BufReader<ChildStdout>>,

    state: ReadState,
    computed: VaspResult,
}

impl Task {
    pub(crate) fn new<P: AsRef<Path>>(exe: P) -> Result<Self> {
        let exe = exe.as_ref();
        let mut child = Command::new(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("run script: {:?}", exe))?;

        let stdout = child.stdout.take();
        let stdin = child.stdin.take();
        let state = ReadState::Skip;
        let computed = VaspResult::default();

        Ok(Self {
            child,
            stdin,
            stdout,
            state,
            computed,
            stdout_reader: None,
        })
    }
}
// base:1 ends here

// [[file:../vasp-server.note::*core][core:1]]
impl Task {
    fn enter_state_read_forces(&mut self) {
        info!("start reading forces");
        self.state = ReadState::Forces;
    }

    fn enter_state_skip(&mut self) {
        self.state = ReadState::Skip;
    }

    fn enter_state_read_energy(&mut self) {
        info!("start reading energy");
        self.state = ReadState::Energy;
    }

    fn enter_state_input_positions(&mut self) {
        info!("read input structure from stdin");
        self.state = ReadState::InputPositions;
    }

    /// Collect result line by line
    fn collect(&mut self, line: &str) {
        self.computed.collect(line, &self.state);
    }

    /// Reset collected results
    fn collect_done(&mut self) {
        self.computed = VaspResult::default();
    }

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

    /// 输出当前结构对应的计算结果
    fn take_action_output(&mut self) -> Result<()> {
        let energy = self.computed.get_energy().expect("no energy");
        let forces = self.computed.get_forces().expect("no forces");

        // FIXME: rewrite
        let mut mp = gosh::model::ModelProperties::default();
        mp.set_forces(dbg!(forces));
        mp.set_energy(dbg!(energy));

        Ok(())
    }

    /// 根据当前状态, 采取对应的行动, 比如继续读取, 输入结构还是输出结果.
    fn take_action(&mut self, line: &str) -> Result<()> {
        dbg!(&line);
        match self.state {
            ReadState::InputPositions => {
                self.take_action_input()?;
                self.enter_state_skip();
            }
            ReadState::Forces => {
                self.collect(line);
            }
            ReadState::Energy => {
                self.collect(line);
                self.take_action_output()?;
                self.collect_done();
            }
            _ => {}
        }

        Ok(())
    }

    /// 开始主循环
    fn enter_main_loop(&mut self) -> Result<()> {
        let mut lines = BufReader::new(self.stdout.take().unwrap()).lines();
        for cycle in 0.. {
            if let Some(line) = lines.next() {
                let line = line?;
                if line == "FORCES:" {
                    self.enter_state_read_forces();
                } else if line.trim_start().starts_with("1 F=") {
                    self.enter_state_read_energy();
                } else if line == "POSITIONS: reading from stdin" {
                    self.enter_state_input_positions();
                }
                self.take_action(&line)?;
            } else {
                break;
            }
        }

        Ok(())
    }
}
// core:1 ends here

// [[file:../vasp-server.note::*vasp][vasp:1]]
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
    if !s.starts_with("FORCES:") {
        None
    } else {
        s.lines()
            .skip(1)
            .map(|line| {
                let parts: Vec<_> = line
                    .split_whitespace()
                    .map(|p| match p.parse() {
                        Ok(value) => value,
                        Err(err) => {
                            dbg!(line, err);
                            todo!();
                        }
                    })
                    .collect();
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

fn parse_energy_and_forces(s: &str) -> Result<(f64, Vec<[f64; 3]>)> {
    let mut lines = s.lines();
    let txt: String = lines
        .skip_while(|line| !line.starts_with("Forces:"))
        .take_while(|line| !line.starts_with("   1 F="))
        .collect();

    todo!();
}
// vasp:1 ends here

// [[file:../vasp-server.note::*compute][compute:1]]
use gosh::gchemol::Molecule;
use gosh::model::ModelProperties;

impl Task {
    pub fn compute_mol(&mut self, mol: &Molecule) -> Result<ModelProperties> {
        // FIXME: rewrite
        let stdout = BufReader::new(self.stdout.take().unwrap());

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
                    let (energy, forces) = parse_energy_and_forces(&text)?;
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
