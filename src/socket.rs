// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use std::os::unix::net::{UnixStream, UnixListener};
use std::path::{Path, PathBuf};
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

    socket_file: Option<server::SocketFile>,
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

// [[file:../vasp-server.note::*codec][codec:1]]
pub(self) mod codec {
    use super::*;
    use bytes::{Buf, BufMut, Bytes};
    use std::io::Read;

    #[derive(Debug, Eq, PartialEq, Clone)]
    /// The request from client side
    pub enum ServerOp {
        /// Write input string into server stream
        Input(String),
        /// Read output from server stream line by line, until we found a line
        /// containing the pattern
        Output(String),
        /// Stop the server
        Stop,
    }

    impl ServerOp {
        /// Encode message ready for sent over UnixStream
        pub fn encode(&self) -> Vec<u8> {
            use ServerOp::*;

            let mut buf = vec![];
            match self {
                Input(msg) => {
                    buf.put_u8(b'0');
                    encode(&mut buf, msg);
                    buf
                }
                Output(pattern) => {
                    buf.put_u8(b'1');
                    encode(&mut buf, pattern);
                    buf
                }
                Stop => {
                    buf.put_u8(b'X');
                    encode(&mut buf, "");
                    buf
                }
                _ => {
                    todo!();
                }
            }
        }

        /// Read and decode raw data as operation for server
        pub fn decode<R: Read>(r: &mut R) -> Result<Self> {
            let mut buf = vec![0_u8; 1];
            r.read_exact(&mut buf)?;
            let mut buf = &buf[..];

            let op = match buf.get_u8() {
                b'0' => {
                    let msg = String::from_utf8_lossy(&decode(r)?).to_string();
                    ServerOp::Input(msg)
                }
                b'1' => {
                    let pattern = String::from_utf8_lossy(&decode(r)?).to_string();
                    ServerOp::Output(pattern)
                }
                b'X' => ServerOp::Stop,
                _ => {
                    todo!();
                }
            };
            Ok(op)
        }
    }

    fn encode<B: BufMut>(mut buf: B, msg: &str) {
        buf.put_u32(msg.len() as u32);
        buf.put(msg.as_bytes());
    }

    fn decode<R: Read>(r: &mut R) -> Result<Vec<u8>> {
        let mut msg = vec![0_u8; 4];
        r.read_exact(&mut msg)?;
        let mut buf = &msg[..];
        let n = buf.get_u32() as usize;
        let mut msg = vec![0_u8; n];
        r.read_exact(&mut msg)?;
        Ok(msg)
    }

    pub fn send_msg(stream: &mut UnixStream, msg: &[u8]) -> Result<()> {
        stream.write_all(msg)?;
        stream.flush()?;
        Ok(())
    }

    pub fn send_msg_encode(stream: &mut UnixStream, msg: &str) -> Result<()> {
        let mut buf = vec![];

        encode(&mut buf, msg);
        send_msg(stream, &buf)?;

        Ok(())
    }

    pub fn recv_msg_decode(stream: &mut UnixStream) -> Result<String> {
        let msg = String::from_utf8_lossy(&decode(stream)?).to_string();
        Ok(msg)
    }

    #[test]
    fn test_codec() -> Result<()> {
        let txt = "hello world\ngood night\n";

        let op = ServerOp::Input(txt.to_string());
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice())?;
        assert_eq!(decoded_op, op);

        let op = ServerOp::Stop;
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice())?;
        assert_eq!(decoded_op, op);

        let pattern = "POSITIONS: reading from stdin".to_string();
        let op = ServerOp::Output(pattern);
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice())?;
        assert_eq!(decoded_op, op);

        Ok(())
    }
}
// codec:1 ends here

// [[file:../vasp-server.note::*client][client:1]]
mod client {
    use super::*;
    use structopt::*;

    pub struct Client {
        stream: UnixStream,
    }

    impl Client {
        /// Make connection to unix domain socket server
        pub fn connect(socket_file: &Path) -> Result<Self> {
            info!("Connect to socket server: {:?}", socket_file);
            let stream = UnixStream::connect(socket_file)?;

            let client = Self { stream };
            Ok(client)
        }

        /// Read output from server line by line until the line containing the
        /// `pattern`
        pub fn read_expect(&mut self, pattern: &str) -> Result<String> {
            info!("Ask for outout from server ...");
            let op = codec::ServerOp::Output(pattern.into());
            self.send_op(op)?;

            debug!("receiving output");
            let txt = codec::recv_msg_decode(&mut self.stream)?;
            debug!("got {} bytes", txt.len());

            Ok(txt)
        }

        /// Write input `msg` into server side
        pub fn write_input(&mut self, msg: &str) -> Result<()> {
            info!("Send input to server ...");
            let op = codec::ServerOp::Input(msg.to_string());
            self.send_op(op)?;

            Ok(())
        }

        fn send_op(&mut self, op: codec::ServerOp) -> Result<()> {
            self.stream.write_all(&op.encode())?;
            self.stream.flush()?;

            Ok(())
        }
    }
}
// client:1 ends here

// [[file:../vasp-server.note::*final version][final version:1]]
mod server {
    use super::*;

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
    }

    impl Drop for SocketFile {
        // clean upunix socket file
        fn drop(&mut self) {
            if self.path.exists() {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    // 启动script进程, 同时将stdin, stdout, stderr都导向stream
    pub fn start_cmd(script: &Path, stream: UnixStream) -> Result<Child> {
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
    pub fn serve_socket(mut server_stream: UnixStream, socket_file: &Path) -> Result<()> {
        use codec::ServerOp;

        info!("serve socket {:?}", socket_file);

        let mut lines = BufReader::new(server_stream.try_clone()?).lines();
        let mut server = SocketFile::create(socket_file)?;
        loop {
            // 1. 等待client发送指令
            server.wait_for_client()?;
            let client_stream = server.stream();
            while let Ok(op) = ServerOp::decode(client_stream) {
                match op {
                    // Write `msg` into server stream
                    ServerOp::Input(msg) => {
                        info!("got input from client");
                        codec::send_msg(&mut server_stream, msg.as_bytes())?;
                    }
                    // Read lines from server_stream until found `pattern`
                    ServerOp::Output(pattern) => {
                        info!("client asks for output");
                        // collect text line by line until we found the `pattern`
                        let mut txt = String::new();
                        while let Some(line) = lines.next() {
                            let line = line?;
                            writeln!(&mut txt, "{}", line)?;
                            if line.contains(&pattern) {
                                break;
                            }
                        }
                        // send colelcted text to client
                        codec::send_msg_encode(client_stream, &txt)?;
                    }
                    ServerOp::Stop => {
                        break;
                    }
                    _ => {
                        todo!();
                    }
                }
            }
        }

        Ok(())
    }
}
// final version:1 ends here

// [[file:../vasp-server.note::*cli][cli:1]]
mod cli {
    use super::*;
    use structopt::*;

    /// VASP calculations server
    #[derive(Debug, StructOpt)]
    struct ServerCli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to script running VASP
        #[structopt(short = "x")]
        script_file: PathBuf,
    }

    pub fn server_enter_main() -> Result<()> {
        let args = ServerCli::from_args();
        args.verbose.setup_logger();

        let socket_file: &Path = SOCKET_FILE.as_ref();
        assert!(!socket_file.exists(), "daemon server already started!");

        // 建新一套通信通道, 将子进程的stdio都导入其中
        let (socket1, mut socket2) = UnixStream::pair()?;

        let mut child = server::start_cmd(&args.script_file, socket1)?;
        server::serve_socket(socket2, socket_file)?;

        child.wait()?;

        Ok(())
    }

    /// Client for VASP server
    #[derive(Debug, StructOpt)]
    struct ClientCli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to script running VASP
        #[structopt(short = "u")]
        socket_file: PathBuf,
    }

    pub fn client_enter_main() -> Result<()> {
        let args = ClientCli::from_args();
        args.verbose.setup_logger();

        let mut client = client::Client::connect(&args.socket_file)?;

        client.write_input("xx\n")?;
        let s = client.read_expect("POSITIONS: reading from stdin")?;
        dbg!(s);

        Ok(())
    }
}
// cli:1 ends here

// [[file:../vasp-server.note::*pub][pub:1]]
pub use self::cli::*;
// pub:1 ends here
