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

// [[file:../vasp-server.note::*client][client:1]]
mod client {
    use super::*;
    use structopt::*;

    struct Client {
        stream: UnixStream,
        reader: BufReader<UnixStream>,
    }

    impl Client {
        // 与socket server建立通信
        fn connect(socket_file: &Path) -> Result<Self> {
            info!("connecting to socket file: {:?}", socket_file);
            let stream = UnixStream::connect(socket_file)?;
            let reader = BufReader::new(stream.try_clone()?);

            let client = Self { stream, reader };
            Ok(client)
        }

        // 从服务端stdout中读取一行出来
        fn read_line(&mut self) -> Result<String> {
            info!("read server output");
            let op = codec::ServerOp::Read;
            self.send_op(op)?;

            let mut txt = String::new();
            self.reader.read_line(&mut txt)?;

            Ok(txt)
        }

        // 将msg写入server端的stdin
        fn write_stdin(&mut self, msg: &str) -> Result<()> {
            info!("write to server");
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

        client.write_stdin("xx\n")?;
        let s = client.read_line()?;
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

// [[file:../vasp-server.note::*codec][codec:1]]
pub(self) mod codec {
    use super::*;
    use bytes::{Buf, BufMut, Bytes};
    use std::io::Read;

    #[derive(Debug, Eq, PartialEq, Clone)]
    // client端发送到server端的指令
    pub enum ServerOp {
        // 将client发来的数据写入stdin
        Input(String),
        // 从stdout中读一行
        Read,
        // 停止子进程
        Stop,
    }

    impl ServerOp {
        pub fn encode(&self) -> Vec<u8> {
            use ServerOp::*;

            let mut buf = vec![];
            match self {
                Input(msg) => {
                    buf.put_u8(b'0');
                    buf.put_u32(msg.len() as u32);
                    buf.put(msg.as_bytes());
                    buf
                }
                Read => {
                    buf.put_u8(b'1');
                    buf.put_u32(0);
                    buf
                }
                Stop => {
                    buf.put_u8(b'X');
                    buf.put_u32(0);
                    buf
                }
                _ => {
                    todo!();
                }
            }
        }

        pub fn decode<R: Read>(r: &mut R) -> Result<Self> {
            let mut buf = vec![0_u8; 5];
            r.read_exact(&mut buf)?;
            let mut buf = &buf[..];

            let op = match buf.get_u8() {
                b'0' => {
                    let n = buf.get_u32() as usize;
                    let mut msg = vec![0; n];
                    r.read_exact(&mut msg)?;
                    let msg = String::from_utf8_lossy(&msg).to_string();
                    ServerOp::Input(msg)
                }
                b'1' => ServerOp::Read,
                b'X' => ServerOp::Stop,
                _ => {
                    todo!();
                }
            };
            Ok(op)
        }
    }

    /// Encode text message into bytes for transfering between unix stream
    pub fn encode(msg: &str) -> Result<Vec<u8>> {
        let mut buf = vec![];
        buf.put_u32(msg.len() as u32);
        buf.put(msg.as_bytes());

        Ok(buf)
    }

    pub fn decode<R: Read>(r: &mut R) -> Result<String> {
        let mut buf = vec![0_u8; 4];
        r.read_exact(&mut buf)?;
        let mut buf = &buf[..];
        let n = buf.get_u32() as usize;

        let mut msg = vec![0; n];
        r.read_exact(&mut msg)?;
        let decoded = String::from_utf8_lossy(&msg);
        Ok(decoded.to_string())
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

        let op = ServerOp::Read;
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice())?;
        assert_eq!(decoded_op, op);

        Ok(())
    }
}
// codec:1 ends here

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

    fn handle_client(op: codec::ServerOp) -> Result<()> {
        todo!()
    }

    // 以socket_file启动server, 将stream里的信息依样处理给client stream
    fn serve_socket(mut server_stream: UnixStream, socket_file: &Path) -> Result<()> {
        use codec::ServerOp;

        info!("serve socket {:?}", socket_file);

        let mut lines = BufReader::new(server_stream.try_clone()?).lines();
        let mut server = SocketFile::create(socket_file)?;
        loop {
            // 1. 等待client发送指令
            server.wait_for_client()?;
            let client_stream = server.stream();
            info!("read instruction from client");
            loop {
                let op = ServerOp::decode(client_stream)?;
                match op {
                    ServerOp::Input(msg) => {
                        server_stream.write_all(msg.as_bytes())?;
                        server_stream.flush()?;
                    }
                    ServerOp::Read => {
                        // 读一行, 写一行
                        log_dbg!();
                        if let Some(line) = lines.next() {
                            log_dbg!();
                            writeln!(client_stream, "{}", dbg!(line?))?;
                            log_dbg!();
                            client_stream.flush()?;
                        }
                    }
                    ServerOp::Stop => {
                        break;
                    }
                    _ => {
                        todo!();
                    }
                }
            }
            break;
        }
        // loop {
        //     // 2. 准备接收server端的输出
        //     log_dbg!();
        //     loop {
        //         log_dbg!();
        //         // 先确定client准备好接收数据
        //         if let Ok(Some(err)) = client_stream.take_error() {
        //             dbg!(err);
        //             break;
        //         }
        //     }
        // }

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
