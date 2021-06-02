// [[file:../vasp-tools.note::*docs][docs:1]]
//! This mod is for VASP interactive calculations.
// docs:1 ends here

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
// base:1 ends here

// [[file:../vasp-tools.note::*task][task:1]]
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

mod task {
    use super::*;

    impl Task {
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
                    // let handler = session.get_handler();
                    if session_handler.is_none() {
                        session_handler = session.spawn()?.into();
                    }
                    assert!(session_handler.is_some());
                    let Interaction(input, read_pattern) = int;
                    let out = session.interact(&input, &read_pattern)?;
                    debug!("coffee break for computation ... {:?}", i);
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

    fn control_session(s: Option<&SessionHandler>, ctl: Control) -> Result<bool> {
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
}
// task:1 ends here

// [[file:../vasp-tools.note::*client][client:1]]
#[derive(Clone)]
pub struct Client {
    // for send client request for pause, resume, stop computation in server side
    tx_ctl: TxControl,
    // for interaction with child process in server side
    tx_int: TxInteraction,
    // for getting child process's stdout running in server side
    rx_out: RxInteractionOutput,
    // for getting notification when computation done in server side
    notifier: Arc<Notify>,
}

mod client {
    use super::*;

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
}
// client:1 ends here

// [[file:../vasp-tools.note::*test][test:1]]
#[cfg(test)]
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

// [[file:../vasp-tools.note::*pub][pub:1]]
/// Create task server and client. The client can be cloned and used in
/// concurrent environment
pub(crate) fn new_interactive_task(program: &Path) -> (Task, Client) {
    let command = Command::new(program);

    let (tx_int, rx_int) = tokio::sync::mpsc::channel(1);
    let (tx_ctl, rx_ctl) = tokio::sync::mpsc::channel(1);
    let (tx_out, rx_out) = tokio::sync::watch::channel("".into());

    let notify1 = Arc::new(Notify::new());
    let notify2 = notify1.clone();
    let session = Session::new(command);
    let server = Task {
        rx_int: rx_int.into(),
        rx_ctl: rx_ctl.into(),
        tx_out: tx_out.into(),
        session: session.into(),
        notifier: notify1,
    };

    let client = Client {
        tx_int,
        tx_ctl,
        rx_out,
        notifier: notify2,
    };

    (server, client)
}
// pub:1 ends here
