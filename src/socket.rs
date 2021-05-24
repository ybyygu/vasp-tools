// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;

use std::os::unix::net::{UnixStream, UnixListener};
use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*codec][codec:1]]
mod codec {
    use super::*;
    use bytes::{Buf, BufMut, Bytes};
    use std::io::{Read, Write};

    /// The request from client side
    #[derive(Debug, Eq, PartialEq, Clone)]
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

// [[file:../vasp-tools.note::*client][client:1]]
mod client {
    use super::*;
    use std::io::{Read, Write};

    /// Client of Unix domain socket
    pub struct Client {
        stream: UnixStream,
    }

    impl Client {
        /// Make connection to unix domain socket server
        pub fn connect(socket_file: &Path) -> Result<Self> {
            info!("Connect to socket server: {:?}", socket_file);
            let stream = UnixStream::connect(socket_file)
                .with_context(|| format!("connect to socket file failure: {:?}", socket_file))?;

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

        /// Try to tell the background computation to stop
        pub fn try_stop(&mut self) -> Result<()> {
            info!("Ask server to stop ...");
            let op = codec::ServerOp::Stop;
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

// [[file:../vasp-tools.note::*server][server:1]]
mod server {
    use super::*;
    use crate::interactive::Task;
    use crate::process::PidFile;
    use std::process::{Child, Command};

    #[derive(Debug)]
    pub struct Server {
        socket_file: PathBuf,
        listener: UnixListener,
        stream: Option<UnixStream>,
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

        fn wait_for_client_stream(&mut self) -> Result<UnixStream> {
            info!("wait for new client");
            let (stream, _) = self.listener.accept().context("accept new unix socket client")?;

            Ok(stream)
        }

        /// Run the `script_file` and serve the client interactions with it
        pub fn run_and_serve(&mut self, program: &Path) -> Result<()> {
            use codec::ServerOp;
            use std::io::BufRead;

            let mut task = None;
            loop {
                // wait for client requests
                let mut client_stream = self.wait_for_client_stream()?;
                while let Ok(op) = ServerOp::decode(&mut client_stream) {
                    match op {
                        // Write `msg` into task's stdin if not empty.
                        ServerOp::Input(msg) => {
                            info!("got input ({} bytes) from client.", msg.len());
                            let task = task.get_or_insert_with(|| create_task(program));
                            if !msg.is_empty() {
                                task.write_stdin(&msg)?;
                            }
                        }
                        // Read task's stdout until the line matching the `pattern`
                        ServerOp::Output(pattern) => {
                            info!("client ask for computed results");
                            if let Some(task) = task.as_mut() {
                                let txt = task.read_stdout_until(&pattern)?;
                                codec::send_msg_encode(&mut client_stream, &txt)?;
                            } else {
                                bail!("Cannot interact with the task's stdout, as it is not started yet!");
                            }
                        }
                        ServerOp::Stop => {
                            info!("client requests to stop computation server");
                            return Ok(());
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

    impl Drop for Server {
        // clean upunix socket file
        fn drop(&mut self) {
            if self.socket_file.exists() {
                let _ = std::fs::remove_file(&self.socket_file);
            }
        }
    }

    fn create_task(program: &Path) -> Task {
        use crate::process::*;

        debug!("run program: {:?}", program);
        use std::process::{Command, Stdio};

        // create child process in a new session, and write session id of the
        // process group, so we can pause/resume/kill theses processes safely
        let child = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .new_process_group()
            .spawn()
            .unwrap();
        Task::new(child, true)
    }
}
// server:1 ends here

// [[file:../vasp-tools.note::*cli][cli:1]]
mod cli {
    use super::*;
    use structopt::*;

    /// A client of a unix domain socket server for interacting with the program
    /// run in background
    #[derive(Debug, StructOpt)]
    struct ClientCli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// Path to the socket file to connect
        #[structopt(short = "u", default_value = "vasp.sock")]
        socket_file: PathBuf,

        /// Stop VASP server
        #[structopt(short = "q")]
        stop: bool,
    }

    pub fn client_enter_main() -> Result<()> {
        let args = ClientCli::from_args();
        args.verbose.setup_logger();

        let mut client = client::Client::connect(&args.socket_file)?;

        if args.stop {
            client.try_stop()?;
        } else {
            // for the first time run, VASP reads coordinates from POSCAR
            if !std::path::Path::new("OUTCAR").exists() {
                info!("Write complete POSCAR file for initial calculation.");
                let txt = crate::vasp::stdin::read_txt_from_stdin()?;
                gut::fs::write_to_file("POSCAR", &txt)?;
                // inform server to start with empty input
                client.write_input("")?;
            } else {
                // redirect scaled positions to server for interactive VASP calculations
                info!("Send scaled coordinates to interactive VASP server.");
                let txt = crate::vasp::stdin::get_scaled_positions_from_stdin()?;
                client.write_input(&txt)?;
            };

            // wait for output
            let s = client.read_expect("POSITIONS: reading from stdin")?;
            let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&s)?;

            let mut mp = gosh::model::ModelProperties::default();
            mp.set_energy(energy);
            mp.set_forces(forces);
            println!("{}", mp);
        }

        Ok(())
    }
}
// cli:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use self::client::*;
pub use self::server::*;
pub use self::cli::*;
// pub:1 ends here
