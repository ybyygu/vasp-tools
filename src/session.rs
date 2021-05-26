// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use tokio::process::{Child, Command};
// imports:1 ends here

// [[file:../vasp-tools.note::*core][core:1]]
/// Manage process session
#[derive(Debug)]
pub struct Session {
    /// Arguments that will be passed to `program`
    rest: Vec<String>,

    /// The external command
    command: Command,

    child: Option<Child>,
}

impl Session {
    /// Create a new session.
    pub fn new(program: &str) -> Self {
        let command = Command::new(program);

        Self {
            command,
            rest: vec![],
            child: None,
        }
    }
}
// core:1 ends here

// [[file:../vasp-tools.note::*signal][signal:1]]
impl Session {
    /// send signal to child processes
    ///
    /// SIGINT, SIGTERM, SIGCONT, SIGSTOP
    fn signal(&self, sig: &str) -> Result<()> {
        if let Some(sid) = self.id() {
            signal_processes_by_session_id(sid, sig)?;
        } else {
            bail!("session not started yet");
        }
        Ok(())
    }

    /// Terminate child processes in a session.
    pub fn terminate(&self) -> Result<()> {
        self.signal("SIGTERM")
    }

    /// Kill processes in a session.
    pub fn kill(&self) -> Result<()> {
        self.signal("SIGKILL")
    }

    /// Resume processes in a session.
    pub fn resume(&self) -> Result<()> {
        self.signal("SIGCONT")
    }

    /// Pause processes in a session.
    pub fn pause(&self) -> Result<()> {
        self.signal("SIGSTOP")
    }

    /// Return session id if child process started.
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|x| x.id())
    }
}

/// Call `pkill` to send signal to related processes
fn signal_processes_by_session_id(sid: u32, signal: &str) -> Result<()> {
    duct::cmd!("pkill", "-s", sid.to_string()).unchecked().run()?;

    Ok(())
}

/// signal processes by session id
fn signal_processes_by_session_id_alt(sid: u32, signal: &str) -> Result<()> {
    // cmdline: kill -CONT -- $(ps -s $1 -o pid=)
    let output = duct::cmd!("ps", "-s", format!("{}", sid), "-o", "pid=").read()?;
    let pids: Vec<_> = output.split_whitespace().collect();

    let mut args = vec!["-s", signal, "--"];
    args.extend(&pids);
    if !pids.is_empty() {
        duct::cmd("kill", &args).unchecked().run()?;
    } else {
        info!("No remaining processes found!");
    }

    Ok(())
}
// signal:1 ends here

// [[file:../vasp-tools.note::*output][output:1]]
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt};

impl Session {
    fn spawn_child(&mut self) -> Result<()> {
        use crate::process::ProcessGroupExt;
        use std::process::Stdio;

        // we want to interact with child process's stdin and stdout
        let child = self
            .command
            .new_process_group()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        // the child process id while it is still running
        self.child = child.into();

        Ok(())
    }
}

async fn read_output_until(mut child: Child, input: &str, pattern: &str) -> Result<String> {
    child
        .stdin
        .take()
        .context("child did not have a handle to stdin")?
        .write_all(input.as_bytes())
        .await
        .context("Failed to write to stdin")?;
    let stdout = child.stdout.take().context("child did not have a handle to stdout")?;
    let mut reader = tokio::io::BufReader::new(stdout).lines();

    tokio::spawn(async move {
        let status = child.wait().await.expect("child process encountered an error");
        eprintln!("child status was: {}", status);
    });

    loop {
        let x = read_until(&mut reader, pattern).await?;
        break Ok(x);
    }
}

async fn read_until<R: AsyncBufRead + Unpin>(reader: &mut tokio::io::Lines<R>, pattern: &str) -> Result<String> {
    let mut text = String::new();
    while let Some(line) = reader.next_line().await? {
        writeln!(&mut text, "{}", line)?;
        if line.starts_with(pattern) {
            return Ok(text);
        }
    }

    bail!("expected pattern not found!");
}
// output:1 ends here

// [[file:../vasp-tools.note::*test][test:1]]
use std::marker::Unpin;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_tokio_child() -> Result<()> {
    use std::process::Stdio;
    use tokio::io::{BufReader, BufWriter};

    let mut tr = Command::new("tr")
        .arg("a-z")
        .arg("A-Z")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = tr.stdin.take().unwrap();
    let w = tokio::spawn(async move {
        write_buffer(&mut stdin, "test\n\n\n").await.unwrap();
        write_buffer(&mut stdin, "test2\n\n\n").await.unwrap();
    });

    let stdout = tr.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let r = tokio::spawn(async move {
        let x = read_line_until(&mut lines, "2").await.unwrap();
        dbg!(x);
    });

    // Ensure the child process is spawned in the runtime so it can
    // make progress on its own while we await for any output.
    let run_proc = tokio::spawn(async move {
        let status = tr.wait().await.expect("child process encountered an error");
        eprintln!("child status was: {}", status);
    });

    let _ = tokio::join!(w, r, run_proc);

    Ok(())
}

// Write `data` into buffer
async fn write_buffer<W: AsyncWriteExt + Unpin>(buffer: &mut W, data: &str) -> Result<()> {
    buffer.write_all(dbg!(data).as_bytes()).await?;
    buffer.flush().await?;

    Ok(())
}

// Read from `reader` until the line containing the `pattern`
async fn read_line_until<R: AsyncBufRead + Unpin>(reader: &mut tokio::io::Lines<R>, pattern: &str) -> Result<String> {
    let mut text = String::new();
    while let Some(line) = reader.next_line().await? {
        writeln!(&mut text, "{}", line)?;
        if dbg!(line).contains(pattern) {
            return Ok(text);
        }
    }

    bail!("expected pattern not found!");
}
// test:1 ends here

// [[file:../vasp-tools.note::*codec][codec:1]]
mod codec {
    use super::*;
    use bytes::{Buf, BufMut, Bytes};
    use std::io::{Read, Write};
    use tokio::io::{AsyncRead, AsyncReadExt};
    use tokio::net::UnixStream;

    /// The request from client side
    #[derive(Debug, Eq, PartialEq, Clone)]
    pub enum ServerOp {
        /// Write input string into server stream
        Input(String),
        /// Read output from server stream line by line, until we found a line
        /// containing the pattern
        Output(String),
        /// Stop the server
        Control(Signal),
    }

    #[derive(Debug, Eq, PartialEq, Clone)]
    pub enum Signal {
        Quit,
        Resume,
        Pause,
    }

    pub type SharedTask = std::sync::Arc<std::sync::Mutex<Session>>;

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
                Control(sig) => {
                    buf.put_u8(b'X');
                    let sig = match sig {
                        Signal::Quit => "SIGTERM",
                        Signal::Resume => "SIGCONT",
                        Signal::Pause => "SIGSTOP",
                    };
                    encode(&mut buf, sig);
                    buf
                }
                _ => {
                    todo!();
                }
            }
        }

        /// Read and decode raw data as operation for server
        pub async fn decode<R: AsyncRead + std::marker::Unpin>(r: &mut R) -> Result<Self> {
            let mut buf = vec![0_u8; 1];
            r.read_exact(&mut buf).await?;
            let mut buf = &buf[..];

            let op = match buf.get_u8() {
                b'0' => {
                    let msg = String::from_utf8_lossy(&decode(r).await?).to_string();
                    ServerOp::Input(msg)
                }
                b'1' => {
                    let pattern = String::from_utf8_lossy(&decode(r).await?).to_string();
                    ServerOp::Output(pattern)
                }
                b'X' => {
                    let sig = String::from_utf8_lossy(&decode(r).await?).to_string();
                    let sig = match sig.as_str() {
                        "SIGTERM" => Signal::Quit,
                        "SIGCONT" => Signal::Resume,
                        "SIGSTOP" => Signal::Pause,
                        _ => todo!(),
                    };
                    ServerOp::Control(sig)
                }
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

    async fn decode<R: AsyncRead + std::marker::Unpin>(r: &mut R) -> Result<Vec<u8>> {
        let mut msg = vec![0_u8; 4];
        r.read_exact(&mut msg).await?;
        let mut buf = &msg[..];
        let n = buf.get_u32() as usize;
        let mut msg = vec![0_u8; n];
        r.read_exact(&mut msg).await?;
        Ok(msg)
    }

    pub async fn send_msg(stream: &mut UnixStream, msg: &[u8]) -> Result<()> {
        stream.write_all(msg).await?;
        stream.flush().await?;
        Ok(())
    }

    pub async fn send_msg_encode(stream: &mut UnixStream, msg: &str) -> Result<()> {
        let mut buf = vec![];

        encode(&mut buf, msg);
        send_msg(stream, &buf).await?;

        Ok(())
    }

    pub async fn recv_msg_decode(stream: &mut UnixStream) -> Result<String> {
        let msg = String::from_utf8_lossy(&decode(stream).await?).to_string();
        Ok(msg)
    }

    #[tokio::test]
    async fn test_async_codec() -> Result<()> {
        let txt = "hello world\ngood night\n";

        let op = ServerOp::Input(txt.to_string());
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice()).await?;
        assert_eq!(decoded_op, op);

        let op = ServerOp::Control(Signal::Quit);
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice()).await?;
        assert_eq!(decoded_op, op);

        let pattern = "POSITIONS: reading from stdin".to_string();
        let op = ServerOp::Output(pattern);
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice()).await?;
        assert_eq!(decoded_op, op);

        Ok(())
    }
}
// codec:1 ends here

// [[file:../vasp-tools.note::*socket][socket:1]]
mod socket {
    use super::*;
    use gut::fs::*;
    use tokio::net::{UnixListener, UnixStream};

    #[derive(Debug)]
    pub struct Server {
        socket_file: PathBuf,
        listener: UnixListener,
        stream: Option<UnixStream>,
    }

    fn remove_socket_file(s: &Path) -> Result<()> {
        if s.exists() {
            std::fs::remove_file(s)?;
        }

        Ok(())
    }

    impl Server {
        async fn handle_contrl_signal(&self) -> Result<()> {
            todo!()
        }

        async fn wait_for_client_stream(&mut self) -> Result<UnixStream> {
            info!("wait for new client");
            let (stream, _) = self.listener.accept().await.context("accept new unix socket client")?;

            Ok(stream)
        }
    }

    impl Drop for Server {
        // clean upunix socket file
        fn drop(&mut self) {
            let _ = remove_socket_file(&self.socket_file);
        }
    }

    impl Server {
        // Create a new socket server. Return error if the server already started.
        pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
            let socket_file = path.as_ref().to_owned();
            if socket_file.exists() {
                bail!("Socket server already started: {:?}!", socket_file);
            }

            let listener = UnixListener::bind(&socket_file).context("bind socket")?;
            info!("serve socket {:?}", socket_file);

            Ok(Server {
                listener,
                socket_file,
                stream: None,
            })
        }

        /// Run the `script_file` and serve the client interactions with it
        pub async fn run_and_serve(&mut self, program: &Path) -> Result<()> {
            use std::sync::{Arc, Mutex};

            // state will be shared with different tasks
            let db = Arc::new(Mutex::new(Session::new("test")));
            loop {
                // wait for client requests
                let mut client_stream = self.wait_for_client_stream().await?;
                // spawn a new task for each client
                let db = db.clone();
                tokio::spawn(async move { handle_client_requests(client_stream, db).await });
            }

            Ok(())
        }
    }

    async fn handle_client_requests(mut client_stream: UnixStream, task: codec::SharedTask) {
        use codec::ServerOp;

        // while let Some(op) = rx.recv().await {
        while let Ok(op) = ServerOp::decode(&mut client_stream).await {
            match op {
                // Write `msg` into task's stdin if not empty.
                ServerOp::Input(msg) => {
                    info!("got input ({} bytes) from client.", msg.len());
                    // let task = task.get_or_insert_with(|| create_task(program));
                    // if !msg.is_empty() {
                    //     task.write_stdin(&msg)?;
                    // }
                }
                // Read task's stdout until the line matching the `pattern`
                ServerOp::Output(pattern) => {
                    info!("client asked for computed results");
                    codec::send_msg_encode(&mut client_stream, "test").await.unwrap();

                    // if let Some(task) = task.as_mut() {
                    //     let txt = task.read_stdout_until(&pattern).await?;
                    //     codec::send_msg_encode(&mut client_stream, &txt).await?;
                    // } else {
                    //     bail!("Cannot interact with the task's stdout, as it is not started yet!");
                    // }
                }
                ServerOp::Control(sig) => {
                    info!("client sent control signal {:?}", sig);
                    return;
                }
                _ => {
                    todo!();
                }
            }
        }
    }
}
// socket:1 ends here

// [[file:../vasp-tools.note::*client][client:1]]
mod client {
    use super::*;
    use gut::fs::*;
    use std::io::{Read, Write};
    use tokio::net::UnixStream;

    /// Client of Unix domain socket
    pub struct Client {
        stream: UnixStream,
    }

    impl Client {
        /// Make connection to unix domain socket server
        pub async fn connect(socket_file: &Path) -> Result<Self> {
            info!("Connect to socket server: {:?}", socket_file);
            let stream = UnixStream::connect(socket_file)
                .await
                .with_context(|| format!("connect to socket file failure: {:?}", socket_file))?;

            let client = Self { stream };
            Ok(client)
        }

        /// Read output from server line by line until the line containing the
        /// `pattern`
        pub async fn read_expect(&mut self, pattern: &str) -> Result<String> {
            info!("Ask for outout from server ...");
            let op = codec::ServerOp::Output(pattern.into());
            self.send_op(op).await?;

            debug!("receiving output");
            let txt = codec::recv_msg_decode(&mut self.stream).await?;
            debug!("got {} bytes", txt.len());

            Ok(txt)
        }

        /// Write input `msg` into server side
        pub async fn write_input(&mut self, msg: &str) -> Result<()> {
            info!("Send input to server ...");
            let op = codec::ServerOp::Input(msg.to_string());
            self.send_op(op).await?;

            Ok(())
        }

        /// Try to tell the background computation to stop
        pub async fn try_quit(&mut self) -> Result<()> {
            self.send_op_control(codec::Signal::Quit).await?;

            Ok(())
        }

        /// Try to tell the background computation to stop
        pub async fn try_pause(&mut self) -> Result<()> {
            self.send_op_control(codec::Signal::Pause).await?;

            Ok(())
        }

        /// Try to tell the background computation to stop
        pub async fn try_resume(&mut self) -> Result<()> {
            self.send_op_control(codec::Signal::Resume).await?;

            Ok(())
        }

        /// Send control signal to server
        async fn send_op_control(&mut self, sig: codec::Signal) -> Result<()> {
            info!("Send control signal {:?}", sig);
            let op = codec::ServerOp::Control(sig);
            self.send_op(op).await?;

            Ok(())
        }

        async fn send_op(&mut self, op: codec::ServerOp) -> Result<()> {
            self.stream.write_all(&op.encode()).await?;
            self.stream.flush().await?;

            Ok(())
        }
    }
}
// client:1 ends here

// [[file:../vasp-tools.note::*server cli][server cli:1]]
mod server_cli {
    use super::*;
    use gut::fs::*;
    use structopt::*;

    /// A client of a unix domain socket server for interacting with the program
    /// run in background
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// The command or the path to invoking VASP program
        #[structopt(short = "x")]
        program: PathBuf,

        /// Path to the socket file to bind (only valid for interactive calculation)
        #[structopt(short = "u", default_value = "vasp.sock")]
        socket_file: PathBuf,
    }

    #[tokio::main]
    pub async fn adhoc_run_vasp_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let mut server = socket::Server::create(&args.socket_file)?;
        // watch for user interruption
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::select! {
            _ = ctrl_c => {
                info!("User interrupted. Shutting down ...");
            },
            _ = server.run_and_serve(&args.program) => {
                todo!();
            }
        }

        Ok(())
    }
}
// server cli:1 ends here

// [[file:../vasp-tools.note::*client cli][client cli:1]]
mod client_cli {
    use super::*;
    use gut::fs::*;
    use structopt::*;

    /// A client of a unix domain socket server for interacting with the program
    /// run in background
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to the socket file to connect
        #[structopt(short = "u", default_value = "vasp.sock")]
        socket_file: PathBuf,

        /// Stop VASP server
        #[structopt(short = "q")]
        stop: bool,
    }

    #[tokio::main]
    pub async fn adhoc_vasp_client_enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let mut client = client::Client::connect(&args.socket_file).await?;
        client.write_input("xx").await?;
        client.read_expect("test").await?;
        client.try_pause().await?;
        client.try_resume().await?;
        client.try_quit().await?;

        Ok(())
    }
}
// client cli:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use client_cli::*;
pub use server_cli::*;
// pub:1 ends here
