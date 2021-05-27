// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::fs::*;
use gut::prelude::*;
use std::path::PathBuf;
use structopt::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*server/adhoc][server/adhoc:1]]
/// A client of a unix domain socket server for interacting with the program
/// run in background
#[derive(Debug, StructOpt)]
struct ServerCli2 {
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
pub async fn run_vasp_enter_main() -> Result<()> {
    use crate::socket::Server;

    let args = ServerCli2::from_args();
    args.verbose.setup_logger();

    let mut server = Server::create(&args.socket_file)?;
    // watch for user interruption
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::select! {
        _ = ctrl_c => {
            info!("User interrupted. Shutting down ...");
        },
        _ = server.run_and_serve(&args.program) => {
            info!("program finished for some reasons.");
        }
    }

    Ok(())
}
// server/adhoc:1 ends here

// [[file:../vasp-tools.note::*client][client:1]]
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

#[tokio::main]
pub async fn vasp_client_enter_main() -> Result<()> {
    use crate::socket::Client;

    let args = ClientCli::from_args();
    args.verbose.setup_logger();

    let mut client = Client::connect(&args.socket_file).await?;
    client.interact("xx", "test").await?;
    client.try_pause().await?;
    client.try_resume().await?;
    client.try_quit().await?;

    Ok(())
}
// client:1 ends here
