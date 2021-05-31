// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
use crate::session::{Session, SessionHandler};

use std::process::Command;
use std::sync::Arc;
use tokio::sync::Notify;
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

type InteractionOutput = String;
type RxInteractionOutput = tokio::sync::watch::Receiver<InteractionOutput>;
type TxInteractionOutput = tokio::sync::watch::Sender<InteractionOutput>;
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
    // notify when computation done
    notifier: Arc<Notify>,
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
        let notifier = self.notifier.clone();
        handle_interaction_new(&mut session, rx_int, tx_out, rx_ctl, notifier).await?;
        Ok(())
    }
}

/// Interact with child process: write stdin with `input` and read in stdout by
/// `read_pattern`
async fn handle_interaction_new(
    session: &mut Session,
    mut rx_int: RxInteraction,
    mut tx_out: TxInteractionOutput,
    mut rx_ctl: RxControl,
    notifier: Arc<Notify>,
) -> Result<()> {
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
                session_handler = Some(Arc::new(handler));
                let Interaction(input, read_pattern) = int;
                let out = session.interact(&input, &read_pattern)?;
                debug!("coffee break for computation ... {:?}", i);
                // tokio::time::sleep(std::time::Duration::from_secs_f64(0.1)).await;
                tx_out.send(out).context("send stdout using tx_out")?;
                &notifier.notify_waiters();
                info!("Computation done: sent client {} the result", i);
            }
            Some(ctl) = rx_ctl.recv() => {
                match control_session(session_handler.as_ref(), ctl) {
                    Ok(false) => {},
                    Ok(true) => break,
                    Err(err) => {
                        error!("control session error: {:?}", err);
                        break;
                    }
                }
            }
            else => {break;}
        };
        info!("Computation done: sent client {} the result", i);
    }

    Ok(())
}

fn control_session(s: Option<&Arc<SessionHandler>>, ctl: Control) -> Result<bool> {
    let s = s.as_ref().ok_or(format_err!("control error: session not started!"))?;

    match ctl {
        Control::Pause => {
            s.pause()?;
        }
        Control::Resume => {
            s.resume()?;
        }
        Control::Quit => {
            s.terminate()?;
            return Ok(true);
        }
    }
    Ok(false)
}
// core:1 ends here

// [[file:../vasp-tools.note::*api][api:1]]
#[derive(Clone)]
pub struct Client {
    tx_ctl: TxControl,
    // for interaction with child process
    tx_int: TxInteraction,
    // for getting child process's stdout
    rx_out: RxInteractionOutput,
    notifier: Arc<Notify>,
}

pub fn new_shared_task(command: Command) -> (Task, Client) {
    let (tx_int, rx_int) = tokio::sync::mpsc::channel(1);
    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel(1);
    let (tx_out, rx_out) = tokio::sync::watch::channel("".into());

    let notify = Arc::new(Notify::new());
    let notify2 = notify.clone();
    let session = Session::new(command);
    let server = Task {
        rx_int: rx_int.into(),
        rx_ctl: rx_ctl.into(),
        tx_out: tx_out.into(),
        session: session.into(),
        notifier: notify,
    };
    let client = Client {
        tx_int,
        tx_ctl,
        rx_out,
        notifier: notify2,
    };

    (server, client)
}

impl Client {
    pub async fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
        // discard the initial value
        // let _ = self.recv_stdout().await?;
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
        self.notifier.notified().await;
        info!("got notification for compuation done");

        if self.rx_out.changed().await.is_ok() {
            let out = &*self.rx_out.borrow();
            Ok(out.to_string())
        } else {
            bail!("todo");
        }
    }
}
// api:1 ends here

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
