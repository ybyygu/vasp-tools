// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use std::os::unix::net::{UnixStream, UnixListener};
use std::path::Path;
// imports:1 ends here

// [[file:../vasp-server.note::*constants][constants:1]]
const SOCKET_FILE: &str = "VASP.sock";
// constants:1 ends here

// [[file:../vasp-server.note::*base][base:1]]
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

use std::io::prelude::*;
use std::io::BufReader;
use std::io::LineWriter;

#[derive(Debug)]
pub(crate) struct Task {
    child: Child,
    stdout: Option<ChildStdout>,
    stdin: Option<ChildStdin>,
    stderr: Option<ChildStderr>,

    socket_file: Option<SocketFile>,
}

impl Task {
    pub(crate) fn new<P: AsRef<Path>>(exe: P) -> Result<Self> {
        let exe = exe.as_ref();
        let mut child = Command::new(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("run script: {:?}", exe))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        Ok(Self {
            child,
            stdin,
            stdout,
            stderr,
            socket_file: None,
        })
    }
}
// base:1 ends here

// [[file:../vasp-server.note::*core][core:1]]
impl Task {
    // 从socket中读取, 并写入子进程的stdin
    fn take_action_input(&mut self) -> Result<()> {
        let socket = self.socket_file.as_mut().expect("no active socket");
        socket.wait_for_client()?;
        let s = socket.recv_input()?;

        let mut writer = std::io::BufWriter::new(self.stdin.as_mut().unwrap());
        writer.write_all(s.as_bytes())?;
        writer.flush()?;

        Ok(())
    }

    // 从子进程中读取stdout, 将写入到socket
    fn take_action_output(&mut self, txt: &str) -> Result<()> {
        let socket = self.socket_file.as_mut().expect("no active socket");
        socket.wait_for_client()?;
        socket.send_output(txt)?;

        Ok(())
    }

    /// 开始主循环
    fn enter_main_loop(&mut self) -> Result<()> {
        // let mut lines = BufReader::new(self.stdout.take().unwrap()).lines();
        // for cycle in 0.. {
        //     if let Some(line) = lines.next() {
        //         let line = line?;
        //         if line == "FORCES:" {
        //             self.enter_state_read_forces();
        //         } else if line.trim_start().starts_with("1 F=") {
        //             self.enter_state_read_energy();
        //         } else if line == "POSITIONS: reading from stdin" {
        //             self.enter_state_input_positions();
        //         }
        //         self.take_action(&line)?;
        //     } else {
        //         break;
        //     }
        // }

        Ok(())
    }
}
// core:1 ends here

// [[file:../vasp-server.note::*client][client:1]]
mod client {
    use super::*;
    use structopt::*;

    struct Client {
        stream: UnixStream,
        reader: BufReader<UnixStream>,
    }

    impl Client {
        fn connect(socket_file: &Path) -> Result<Self> {
            info!("connecting to socket file: {:?}", socket_file);
            let stream = UnixStream::connect(socket_file)?;
            let reader = BufReader::new(stream.try_clone()?);

            let client = Self { stream, reader };
            Ok(client)
        }

        fn read(&mut self) -> Result<String> {
            info!("read server output");
            let mut txt = String::new();
            while let Some(_) = self.reader.read_line(&mut txt).ok().filter(|&x| x != 0) {
                dbg!(&txt);
            }
            Ok(txt)
        }

        fn write(&mut self, msg: &str) -> Result<()> {
            info!("write to server");
            let _ = self.stream.write_all(msg.as_bytes())?;
            self.stream.flush()?;

            Ok(())
        }
    }

    fn read_output_from_socket_file(socket_file: &Path) -> Result<String> {
        info!("connecting to socket file: {:?}", socket_file);

        let mut stream = UnixStream::connect(socket_file)?;
        let mut output = String::new();
        stream.read_to_string(&mut output)?;

        Ok(output)
    }

    /// Client for VASP server
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to script running VASP
        #[structopt(short = "u")]
        socket_file: PathBuf,
    }

    pub fn client_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let mut client = Client::connect(&args.socket_file)?;

        client.write("xx\n")?;
        let s = client.read()?;
        dbg!(s);

        Ok(())
    }
}
// client:1 ends here

// [[file:../vasp-server.note::*server][server:1]]
use std::path::PathBuf;

#[derive(Debug)]
pub struct SocketFile {
    path: PathBuf,
    listener: UnixListener,
    stream: Option<UnixStream>,
}

impl SocketFile {
    // Create a new VASP server. Return error if the server already started.
    fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
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
        info!("wait for new client");
        let (stream, _) = self.listener.accept().context("accept new unix socket client")?;
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

// [[file:../vasp-server.note::*server][server:1]]
mod server {
    use super::*;
    use structopt::*;

    /// VASP calculations server
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to script running VASP
        #[structopt(short = "x")]
        script_file: PathBuf,
    }

    // 启动script进程, 同时将stdin, stdout, stderr都导向stream
    fn start_cmd(script: &Path, stream: UnixStream) -> Result<Child> {
        info!("run script: {:?}", script);
        
        use std::os::unix::io::{AsRawFd, FromRawFd};

        let mut i_stream = stream.try_clone()?;
        let mut o_stream = stream.try_clone()?;
        let mut e_stream = stream.try_clone()?;

        // make unix stream as file descriptors for process stdio
        let (i_cmd, o_cmd, e_cmd) = unsafe {
            use std::process::Stdio;
            (
                Stdio::from_raw_fd(i_stream.as_raw_fd()),
                Stdio::from_raw_fd(o_stream.as_raw_fd()),
                Stdio::from_raw_fd(e_stream.as_raw_fd()),
            )
        };

        let child = Command::new(script).stdin(i_cmd).stdout(o_cmd).stderr(e_cmd).spawn()?;

        Ok(child)
    }

    // 以socket_file启动server, 将stream里的信息依样处理给client stream
    fn serve_socket(mut server_stream: UnixStream, socket_file: &Path) -> Result<()> {
        info!("serve socket {:?}", socket_file);
        
        let mut lines = BufReader::new(server_stream.try_clone()?).lines();
        let mut server = SocketFile::create(socket_file)?;
        loop {
            server.wait_for_client()?;
            let client_stream = server.stream();
            // 1. 接收client input
            let mut text = String::new();
            info!("read str from client");
            let _ = client_stream.read_to_string(&mut text)?;
            info!("write client input to server");
            server_stream.write_all(dbg!(text).as_bytes())?;

            while let Some(line) = lines.next() {
                let line = line?;
                client_stream.write_all(dbg!(line).as_bytes()).context("write server output")?;
                client_stream.flush()?;
                
            }
        }

        Ok(())
    }

    pub fn server_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let socket_file: &Path = SOCKET_FILE.as_ref();
        assert!(!socket_file.exists(), "daemon server already started!");

        // 建新一套通信通道, 将子进程的stdio都导入其中
        let (socket1, mut socket2) = UnixStream::pair()?;

        let mut child = start_cmd(&args.script_file, socket1)?;
        serve_socket(socket2, socket_file)?;

        child.wait()?;

        Ok(())
    }
}
// server:1 ends here

// [[file:../vasp-server.note::*pub][pub:1]]
pub use self::client::*;
pub use self::server::*;
// pub:1 ends here
