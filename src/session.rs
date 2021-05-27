// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*core/rexpect][core/rexpect:1]]
use rexpect::session::PtySession;
use std::process::{ChildStdin, ChildStdout, Command};

/// Return child processes in a session group
pub struct Session {
    command: Option<Command>,
    session: Option<PtySession>,
}

impl Session {
    /// Create a new session for running `command`
    pub fn new(command: Command) -> Self {
        Self {
            command: command.into(),
            session: None,
        }
    }

    /// Return child process's session ID, useful for killing all child
    /// processes using `pkill` command.
    pub fn id(&self) -> Option<u32> {
        let sid = self.session.as_ref()?.process.child_pid.as_raw();

        Some(sid as u32)
    }

    /// Interact with child process's stdin using `input` and return stdout
    /// read-in until the line matching `read_pattern`
    pub fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
        use rexpect::ReadUntil;

        // create a new session for the first time
        if self.session.is_none() {
            let command = self.command.take().unwrap();
            self.session = create_new_session(command)?.into();
            info!("start child process in new session: {:?}", self.id());
        }
        let s = self.session.as_mut().expect("rexpect session");

        trace!("send input for child process's stdin");
        s.send_line(input)
            .map_err(|e| format_err!("send input error: {:?}", e))?;

        trace!("send read pattern for child process's stdout");
        let (x, _) = s
            .exp_any(vec![ReadUntil::String(read_pattern.into()), ReadUntil::EOF])
            .map_err(|e| format_err!("read stdout error: {:?}", e))?;
        return Ok(x);

        bail!("invalid stdin/stdout!");
    }
}

/// Spawn child process in a new session
fn create_new_session(command: Command) -> Result<PtySession> {
    use rexpect::session::spawn_command;

    let session = spawn_command(command, None).map_err(|e| format_err!("spawn command error: {:?}", e))?;

    Ok(session)
}

#[test]
fn test_session_interact() -> Result<()> {
    gut::cli::setup_logger_for_test();

    let sh = std::process::Command::new("tests/files/interactive-job.sh");
    let mut s = Session::new(sh);

    let o = s.interact("test1\n", "POSITIONS: reading from stdin")?;
    assert!(o.contains("mag=     2.2094"));
    let o = s.interact("test1\n", "POSITIONS: reading from stdin")?;
    assert!(o.contains("mag=     2.3094"));

    Ok(())
}
// core/rexpect:1 ends here

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

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Session {
    fn drop(&mut self) {
        if let Some((sid, status)) = self.id().zip(self.status()) {
            dbg!(sid, status);
            // self.terminate();
        }
    }
}

impl Session {
    fn status(&self) -> Option<rexpect::process::wait::WaitStatus> {
        let status = self.session.as_ref()?.process.status()?;
        status.into()
    }
}
// drop:1 ends here

// [[file:../vasp-tools.note::*codec][codec:1]]
/// Shared codes for both server and client sides
mod codec {
    use super::*;
    use bytes::{Buf, BufMut, Bytes};
    use std::io::{Read, Write};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    pub type SharedSession = std::sync::Arc<std::sync::Mutex<Session>>;

    pub fn new_shared_session(command: Command) -> SharedSession {
        use std::sync::{Arc, Mutex};
        Arc::new(Mutex::new(Session::new(command)))
    }

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

        /// Run the `program` backgroundly and serve the client interactions with it
        pub async fn run_and_serve(&mut self, program: &Path) -> Result<()> {
            use std::sync::{Arc, Mutex};

            // state will be shared with different tasks
            let command = Command::new(program);
            let db = codec::new_shared_session(command);
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

    async fn handle_client_requests(mut client_stream: UnixStream, task: codec::SharedSession) {
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
            use tokio::io::AsyncWriteExt;

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
