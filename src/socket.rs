// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::session::Session;
use gut::prelude::*;
use std::process::Command;
// imports:1 ends here

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
        /// Control server process: pause/resume/quit
        Control(Signal),
        /// Interact with server process with input for stdin and read-pattern for stdout.
        Interact((String, String)),
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
                Interact((input, pattern)) => {
                    buf.put_u8(b'0');
                    encode(&mut buf, input);
                    encode(&mut buf, pattern);
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
                    let input = String::from_utf8_lossy(&decode(r).await?).to_string();
                    let pattern = String::from_utf8_lossy(&decode(r).await?).to_string();
                    ServerOp::Interact((input, pattern))
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
        let op = ServerOp::Control(Signal::Quit);
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice()).await?;
        assert_eq!(decoded_op, op);

        let input = "hello world\ngood night\n".to_string();
        let pattern = "POSITIONS: reading from stdin".to_string();
        let op = ServerOp::Interact((input, pattern));
        let d = op.encode();
        let decoded_op = ServerOp::decode(&mut d.as_slice()).await?;
        assert_eq!(decoded_op, op);

        Ok(())
    }
}
// codec:1 ends here

// [[file:../vasp-tools.note::*server][server:1]]
mod server {
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
        // clean up existing unix domain socket file
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
                ServerOp::Interact((input, pattern)) => {
                    info!("client asked for interaction with input and read-pattern");
                    let mut task = task.lock().unwrap();
                    if let Err(e) = task.interact(&input, &pattern) {
                        error!("interaction failure: {:?}", e);
                        break;
                    }
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
// server:1 ends here

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

        /// Interact with background server using `input` for stdin and
        /// `read_pattern` for reading stdout.
        pub async fn interact(&mut self, input: &str, read_pattern: &str) -> Result<()> {
            info!("Interact with server process ...");
            let op = codec::ServerOp::Interact((input.to_string(), read_pattern.to_string()));
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

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use client::Client;
pub use server::Server;
// pub:1 ends here
