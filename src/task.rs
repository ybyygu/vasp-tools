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

struct Task {
    child: Child,
    stdout: Option<ChildStdout>,
    stdin: Option<ChildStdin>,

    state: ReadState,
    computed: VaspResult,

    socket_file: Option<SocketFile>,
}

impl Task {
    fn new<P: AsRef<Path>>(exe: P) -> Result<Self> {
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
            socket_file: None,
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
        let socket = self.socket_file.as_mut().expect("no active socket");
        socket.wait_for_client()?;
        socket.send_output(&mp.to_string())?;

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
        loop {
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
// vasp:1 ends here

// [[file:../vasp-server.note::*client][client:1]]
fn read_output_from_socket_file(socket_file: &Path) -> Result<String> {
    info!("connecting to socket file: {:?}", socket_file);

    let mut stream = UnixStream::connect(socket_file)?;
    let mut output = String::new();
    stream.read_to_string(&mut output)?;

    Ok(output)
}
// client:1 ends here

// [[file:../vasp-server.note::*server][server:1]]
use std::path::PathBuf;

#[derive(Debug)]
pub struct SocketFile {
    path: PathBuf,
    listener: std::os::unix::net::UnixListener,
    stream: Option<std::os::unix::net::UnixStream>,
}

impl SocketFile {
    // Create a new VASP server. Return error if the server already started.
    fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        use std::os::unix::net::UnixListener;

        let path = path.as_ref();
        if path.exists() {
            bail!("VASP server already started!");
        }

        let listener = UnixListener::bind(&path).with_context(|| format!("bind to socket file: {:?}", &path))?;
        Ok(SocketFile {
            listener,
            path: path.to_owned(),
            stream: None,
        })
    }

    fn wait_for_client(&mut self) -> Result<()> {
        let (stream, _) = self.listener.accept()?;
        self.stream = stream.into();

        Ok(())
    }

    fn stream(&mut self) -> &mut UnixStream {
        self.stream.as_mut().expect("unix stream not ready")
    }

    /// 将`out`发送给client
    fn send_output(&mut self, out: &str) -> Result<()> {
        debug!("send out to client ...");
        write!(self.stream(), "{}", out);
        debug!("shutdown socket stream ...");
        self.stream().shutdown(std::net::Shutdown::Both)?;

        Ok(())
    }

    /// 向client请求输入新的结构
    fn recv_input(&mut self) -> Result<String> {
        let mut inputs = String::new();
        let nbytes = self.stream().read_to_string(&mut inputs)?;
        assert_ne!(nbytes, 0);

        Ok(inputs)
    }
}

impl Drop for SocketFile {
    // clean upunix socket file
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
// server:1 ends here

// [[file:../vasp-server.note::*cli][cli:1]]
use structopt::*;

mod client {
    use super::*;

    /// Client for VASP server
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,
    }

    pub fn client_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let s = read_output_from_socket_file(SOCKET_FILE.as_ref())?;

        println!("{}", s);

        Ok(())
    }
}

mod server {
    use super::*;

    /// VASP calculations server
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to script running VASP
        #[structopt(short = "x")]
        script_file: PathBuf,
    }

    pub fn server_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let socket_file: &Path = SOCKET_FILE.as_ref();
        assert!(!socket_file.exists(), "daemon server already started!");

        let mut task = Task::new(&args.script_file)?;
        // FIXME: rewrite the next line
        task.socket_file = SocketFile::create(SOCKET_FILE)?.into();

        // Start VASP server in background if not started yet
        // FIXME: rewrite using daemon
        task.enter_main_loop()?;

        Ok(())
    }
}

pub use self::client::*;
pub use self::server::*;
// cli:1 ends here
