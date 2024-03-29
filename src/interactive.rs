// [[file:../vasp-tools.note::*docs][docs:1]]
//! This mod is for VASP interactive calculations.
// docs:1 ends here

// [[file:../vasp-tools.note::0bd38257][0bd38257]]
use super::*;
use crate::session::{Session, SessionHandler};

use std::process::Command;
use std::sync::Arc;
use tokio::sync::Notify;
// 0bd38257 ends here

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
// base:1 ends here

// [[file:../vasp-tools.note::0236f1ec][0236f1ec]]
pub struct TaskServer {
    // for receiving interaction message for child process
    rx_int: Option<RxInteraction>,
    // for controlling child process
    rx_ctl: Option<RxControl>,
    // for sending child process's stdout
    tx_out: Option<TxInteractionOutput>,
    // notify when computation done
    notifier: Arc<Notify>,
    // child process
    session: Option<Session>,
}

mod taskserver {
    use super::*;

    impl TaskServer {
        /// Run child process in new session, and serve requests for interactions.
        pub async fn run_and_serve(&mut self) -> Result<()> {
            let mut session = self.session.as_mut().context("no running session")?;
            let rx_int = self.rx_int.take().context("no rx_int")?;
            let rx_ctl = self.rx_ctl.take().context("no rx_ctl")?;
            let tx_out = self.tx_out.take().context("no tx_out")?;
            let notifier = self.notifier.clone();
            handle_interaction(&mut session, rx_int, tx_out, rx_ctl, notifier).await?;
            Ok(())
        }
    }

    /// Interact with child process: write stdin with `input` and read in stdout by
    /// `read_pattern`
    async fn handle_interaction(
        session: &mut Session,
        mut rx_int: RxInteraction,
        mut tx_out: TxInteractionOutput,
        mut rx_ctl: RxControl,
        notifier: Arc<Notify>,
    ) -> Result<()> {
        let mut session_handler = session.get_handler();
        for i in 0.. {
            tokio::select! {
                Some(int) = rx_int.recv() => {
                    if session_handler.is_none() {
                        session_handler = session.spawn()?.into();
                    }
                    assert!(session_handler.is_some());
                    let Interaction(input, read_pattern) = int;
                    let out = session.interact(&input, &read_pattern)?;
                    debug!("coffee break for computation ... {:?}", i);
                    tx_out.send(out).context("send stdout using tx_out")?;
                    &notifier.notify_waiters();
                    debug!("Computation done: sent client {} the result", i);
                }
                Some(ctl) = rx_ctl.recv() => {
                    match break_control_session(session_handler.as_ref(), ctl) {
                        Ok(false) => {},
                        Ok(true) => break,
                        Err(err) => {error!("control session error: {:?}", err); break;}
                    }
                }
                else => {
                    bail!("Unexpected branch: the communication channels broken?");
                }
            };
        }

        Ok(())
    }

    fn break_control_session(s: Option<&SessionHandler>, ctl: Control) -> Result<bool> {
        let s = s.as_ref().ok_or(format_err!("control error: session not started!"))?;

        match ctl {
            Control::Pause => s.pause()?,
            Control::Resume => s.resume()?,
            Control::Quit => {
                s.terminate()?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}
// 0236f1ec ends here

// [[file:../vasp-tools.note::d0da5283][d0da5283]]
#[derive(Clone)]
pub struct TaskClient {
    // for send client request for pause, resume, stop computation on server side
    tx_ctl: TxControl,
    // for interaction with child process on server side
    tx_int: TxInteraction,
    // for getting child process's stdout running on server side
    rx_out: RxInteractionOutput,
    // for getting notification when computation done on server side
    notifier: Arc<Notify>,
}

mod taskclient {
    use super::*;

    impl TaskClient {
        pub async fn interact(&mut self, input: &str, read_pattern: &str) -> Result<String> {
            self.tx_int.send(Interaction(input.into(), read_pattern.into())).await?;
            let out = self.recv_stdout().await?;
            Ok(out)
        }

        pub async fn pause(&self) -> Result<()> {
            trace!("send pause task msg");
            self.tx_ctl.send(Control::Pause).await?;
            Ok(())
        }

        pub async fn resume(&self) -> Result<()> {
            trace!("send resume task msg");
            self.tx_ctl.send(Control::Resume).await?;
            Ok(())
        }

        pub async fn terminate(&self) -> Result<()> {
            trace!("send quit task msg");
            self.tx_ctl.send(Control::Quit).await?;
            Ok(())
        }

        /// return the output already read in from child process's stdout
        async fn recv_stdout(&mut self) -> Result<String> {
            // wait for server's notification for job done
            self.notifier.notified().await;
            // read stdout from the channel
            self.rx_out.changed().await?;
            let out = &*self.rx_out.borrow();
            Ok(out.to_string())
        }
    }
}
// d0da5283 ends here

// [[file:../vasp-tools.note::564109b4][564109b4]]
/// Create task server and client. The client can be cloned and used in
/// concurrent environment
pub fn new_interactive_task(program: &Path) -> (TaskServer, TaskClient) {
    let command = Command::new(program);

    let (tx_int, rx_int) = tokio::sync::mpsc::channel(1);
    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel(1);
    let (tx_out, rx_out) = tokio::sync::watch::channel("".into());

    let notify1 = Arc::new(Notify::new());
    let notify2 = notify1.clone();
    let session = Session::new(command);

    let server = TaskServer {
        rx_int: rx_int.into(),
        rx_ctl: rx_ctl.into(),
        tx_out: tx_out.into(),
        session: session.into(),
        notifier: notify1,
    };

    let client = TaskClient {
        tx_int,
        tx_ctl,
        rx_out,
        notifier: notify2,
    };

    (server, client)
}
// 564109b4 ends here

// [[file:../vasp-tools.note::*test][test:1]]
#[cfg(test)]
mod test {
    use super::*;

    async fn handle_vasp_interaction(task: &mut TaskClient) -> Result<()> {
        let input = include_str!("../tests/files/interactive_positions.txt");
        let read_pattern = "POSITIONS: reading from stdin";
        let out = task.interact(&input, read_pattern).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_task1() -> Result<()> {
        gut::cli::setup_logger_for_test();

        // test control signal
        let (mut server, mut client) = new_interactive_task("fake-vasp".as_ref());
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
        let (mut server, mut client) = new_interactive_task("fake-vasp".as_ref());

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
