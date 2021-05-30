// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
use std::process::Command;
// use crate::session::Session;
use session::{Session, SessionHandler};
// imports:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
#[derive(Debug, Clone)]
struct Interaction(String, String);

/// The message sent from client for controlling child process
#[derive(Debug, Clone)]
enum Control {
    Quit,
    Pause,
    Resume,
}

#[derive(Debug, Clone)]
enum InteractionProgress {
    Output(String),
    Running(SessionHandler),
}

// struct InteractionProgress(String, u32);
// type InteractionProgress = String;
type RxInteractionOutput = tokio::sync::watch::Receiver<InteractionProgress>;
type TxInteractionOutput = tokio::sync::watch::Sender<InteractionProgress>;
type RxInteraction = tokio::sync::mpsc::Receiver<Interaction>;
type TxInteraction = tokio::sync::mpsc::Sender<Interaction>;
type RxControl = tokio::sync::mpsc::Receiver<Control>;
type TxControl = tokio::sync::mpsc::Sender<Control>;

pub struct Task {
    // for receiving interaction message for child process
    rx_int: Option<RxInteraction>,
    // for controlling child process
    rx_ctl: Option<RxControl>,
    // for sending child process's stdout
    tx_out: Option<TxInteractionOutput>,
    // child process
    session: Option<Session>,
}
// base:1 ends here

// [[file:../vasp-tools.note::*core][core:1]]
impl Task {
    /// Run child process in new session, and serve requests for interactions.
    pub async fn run_and_serve(&mut self) -> Result<()> {
        let mut session = self.session.as_mut().context("no running session")?;
        let rx_int = self.rx_int.take().context("no rx_int")?;
        let rx_ctl = self.rx_ctl.take().context("no rx_ctl")?;
        let tx_out = self.tx_out.take().context("no tx_out")?;

        handle_interaction_new(&mut session, rx_int, tx_out, rx_ctl).await?;
        Ok(())
    }
}

/// Interact with child process: write stdin with `input` and read in stdout by
/// `read_pattern`
async fn handle_interaction(
    session: &mut Session,
    mut rx_int: RxInteraction,
    mut tx_out: TxInteractionOutput,
) -> Result<()> {
    for i in 0.. {
        // get parameters for interaction with child process
        let Interaction(input, read_pattern) = rx_int
            .recv()
            .await
            .ok_or(format_err!("Interaction channel was closed."))?;
        info!("Received client {} interation request", i);

        // the session is started
        let handler = if let Some(sid) = session.id() {
            SessionHandler::new(sid)
        } else {
            let sid = session.spawn_new()?;
            SessionHandler::new(sid)
        };
        tx_out.send(InteractionProgress::Running(handler))?;
        let out = session.interact(&input, &read_pattern)?;
        info!("Computation done: sent client {} the result ({} bytes)", i, out.len());
        // we get interaction output
        let int_out = InteractionProgress::Output(out);
        tx_out.send(int_out).context("send stdout using tx_out")?;
    }

    Ok(())
}

/// Interact with child process: write stdin with `input` and read in stdout by
/// `read_pattern`
async fn handle_interaction_new(
    session: &mut Session,
    mut rx_int: RxInteraction,
    mut tx_out: TxInteractionOutput,
    mut rx_ctl: RxControl,
) -> Result<()> {
    use std::sync::Arc;

    let mut session_handler = None;
    for i in 0.. {
        tokio::select! {
            Some(int) = rx_int.recv() => {
                let handler = if let Some(sid) = session.id() {
                    SessionHandler::new(sid)
                } else {
                    let sid = session.spawn_new()?;
                    SessionHandler::new(sid)
                };
                session_handler = Some(std::sync::Arc::new(handler));
                let Interaction(input, read_pattern) = int;
                let out = session.interact(&input, &read_pattern)?;
                let int_out = InteractionProgress::Output(out);
                tx_out.send(int_out).context("send stdout using tx_out")?;
                info!("Computation done: sent client {} the result", i);
            }
            Some(ctl) = rx_ctl.recv() => {
                match dbg!(ctl) {
                    Control::Pause =>  {session_handler.as_ref().unwrap().pause();}
                    Control::Resume =>  {session_handler.as_ref().unwrap().resume();}
                    Control::Quit =>  {session_handler.as_ref().unwrap().terminate();}
                }
            }
            else => {break;}
        };
        info!("Computation done: sent client {} the result", i);
    }

    Ok(())
}
// core:1 ends here

// [[file:../vasp-tools.note::*session][session:1]]
mod session {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::Command;
    use std::process::{Child, ChildStdin, ChildStdout};

    /// Run child processes in a new session group for easy control
    pub struct Session {
        command: Option<Command>,
        session: Option<Child>,
        stream0: Option<ChildStdin>,
        stream1: Option<std::io::Lines<BufReader<ChildStdout>>>,
    }

    /// Spawn child process in a new session
    fn create_new_session(mut command: Command) -> Result<Child> {
        use crate::process::ProcessGroupExt;
        use std::process::Stdio;

        // we want to interact with child process's stdin and stdout
        let child = command
            .new_process_group()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        Ok(child)
    }

    impl Session {
        /// Create a new session for running `command`
        pub fn new(command: Command) -> Self {
            Self {
                command: command.into(),
                session: None,
                stream0: None,
                stream1: None,
            }
        }

        pub fn quit(&mut self) -> Result<()> {
            info!("TODO: quit session");
            Ok(())
        }

        /// Interact with child process's stdin using `input` and return stdout
        /// read-in until the line matching `read_pattern`. The child process will
        /// be automatically spawned if necessary.
        pub fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
            use std::io::prelude::*;

            let s = self.session.as_mut().expect("rexpect session not started yet");

            // ignore interaction with empty input
            let stdin = self.stream0.as_mut().unwrap();
            if !input.is_empty() {
                trace!("send input for child process's stdin ({} bytes)", input.len());
                stdin.write_all(input.as_bytes())?;
                stdin.flush()?;
            }
            trace!("send read pattern for child process's stdout: {:?}", read_pattern);

            let mut txt = String::new();
            let stdout = self.stream1.as_mut().unwrap();
            for line in stdout {
                let line = line?;
                writeln!(&mut txt, "{}", line)?;
                if line.starts_with(read_pattern) {
                    break;
                }
            }

            if txt.is_empty() {
                bail!("Got nothing for pattern: {}", read_pattern);
            }
            return Ok(txt);
        }

        /// Return child process's session ID, useful for killing all child
        /// processes using `pkill` command.
        pub fn id(&self) -> Option<u32> {
            self.session.as_ref().map(|s| s.id())
        }

        pub(super) fn spawn_new(&mut self) -> Result<u32> {
            let command = self.command.take().unwrap();
            let mut child = create_new_session(command)?;
            self.stream0 = child.stdin.take().unwrap().into();
            let stdout = child.stdout.take().unwrap();
            self.stream1 = BufReader::new(stdout).lines().into();
            self.session = child.into();

            let pid = self.id().unwrap();
            info!("start child process in new session: {:?}", pid);
            Ok(pid)
        }
    }

    #[derive(Debug, Clone)]
    pub(super) struct SessionHandler {
        pid: u32,
    }

    impl SessionHandler {
        /// Create a SessionHandler for process `pid`
        pub fn new(pid: u32) -> Self {
            Self { pid }
        }
    }

    impl SessionHandler {
        /// send signal to child processes: SIGINT, SIGTERM, SIGCONT, SIGSTOP
        fn signal(&self, sig: &str) -> Result<()> {
            info!("signal process {} with {}", self.pid, sig);
            signal_processes_by_session_id(self.pid, sig)?;
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
        debug!("kill session {} using signal {:?}", sid, signal);
        duct::cmd!("pkill", "-s", sid.to_string()).unchecked().run()?;

        Ok(())
    }
}
// session:1 ends here

// [[file:../vasp-tools.note::*drop][drop:1]]
impl Drop for Task {
    fn drop(&mut self) {
        if let Some(s) = self.session.as_mut() {
            if let Err(e) = s.quit() {
                dbg!(e);
            }
        }
    }
}
// drop:1 ends here

// [[file:../vasp-tools.note::*client][client:1]]
#[derive(Clone)]
pub struct Client {
    session: Option<SessionHandler>,
    tx_ctl: TxControl,
    // for interaction with child process
    tx_int: TxInteraction,
    // for getting child process's stdout
    rx_out: RxInteractionOutput,
}

pub fn new_shared_task(command: Command) -> (Task, Client) {
    let (tx_int, rx_int) = tokio::sync::mpsc::channel(1);
    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel(1);
    let (tx_out, rx_out) = tokio::sync::watch::channel(InteractionProgress::Output("".into()));

    let session = Session::new(command);
    let server = Task {
        rx_int: rx_int.into(),
        rx_ctl: rx_ctl.into(),
        tx_out: tx_out.into(),
        session: session.into(),
    };
    let client = Client {
        session: None,
        tx_int,
        tx_ctl,
        rx_out,
    };

    (server, client)
}
// client:1 ends here

// [[file:../vasp-tools.note::*v2][v2:1]]
impl Client {
    pub async fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
        self.tx_int.send(Interaction(input.into(), read_pattern.into())).await?;
        let out = self.recv_stdout().await?;
        Ok(out)
    }

    pub async fn pause(&self) -> Result<()> {
        info!("send pause task msg");
        self.tx_ctl.send(Control::Pause).await?;
        Ok(())
    }

    pub async fn resume(&self) -> Result<()> {
        info!("send resume task msg");
        self.tx_ctl.send(Control::Resume).await?;
        Ok(())
    }

    pub async fn terminate(&self) -> Result<()> {
        info!("send quit task msg");
        self.tx_ctl.send(Control::Quit).await?;
        Ok(())
    }

    /// return the output already read in from child process's stdout
    async fn recv_stdout(&mut self) -> Result<String> {
        // continue to receive until we get the output
        loop {
            if self.rx_out.changed().await.is_ok() {
                match &*self.rx_out.borrow() {
                    InteractionProgress::Output(out) => {
                        info!("got output ({} bytes)", out.len());
                        return Ok(out.to_string());
                    }
                    InteractionProgress::Running(session) => {
                        info!("session started: {:?}", session);
                        self.session = session.clone().into();
                    }
                }
            } else {
                bail!("tx handler has been dropped!");
            }
        }
    }
}
// v2:1 ends here

// [[file:../vasp-tools.note::*test][test:1]]
mod test {
    use super::*;

    async fn handle_vasp_interaction(task: &mut Client) -> Result<()> {
        let input = include_str!("../tests/files/interactive_positions.txt");
        let read_pattern = "POSITIONS: reading from stdin";
        let out = task.interact(&input, read_pattern).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_task1() -> Result<()> {
        gut::cli::setup_logger_for_test();

        // test control signal
        let command = Command::new("fake-vasp");
        let (mut server, mut client) = new_shared_task(command);
        tokio::spawn(async move {
            server.run_and_serve().await.unwrap();
        });
        handle_vasp_interaction(&mut client).await?;
        client.resume().await?;
        client.pause().await?;
        client.terminate().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_task2() -> Result<()> {
        gut::cli::setup_logger_for_test();
        let command = Command::new("fake-vasp");
        let (mut server, mut client) = new_shared_task(command);

        // start the server side
        let h = server.run_and_serve();
        // set timeout for breaking the loop
        let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(1));
        // for resuming async operations: https://tokio.rs/tokio/tutorial/select
        tokio::pin!(h);
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    info!("Timeout reached!");
                    break;
                },
                res = &mut h => {
                    if let Err(e) = res {
                        error!("Task server error: {:?}", e);
                    }
                },
                _ = async {
                    let mut task = client.clone();
                    if let Err(e) = handle_vasp_interaction(&mut task).await {
                        error!("Task client failure: {:?}", e);
                    }
                } => {},
            }
        }

        Ok(())
    }
}
// test:1 ends here
