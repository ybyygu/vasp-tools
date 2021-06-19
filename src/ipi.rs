// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;

use gosh::gchemol::Molecule;
// imports:1 ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod codec;
// mods:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
/// The Message type sent from client side (the computation engine)
#[derive(Debug, Clone, PartialEq)]
pub enum ClientStatus {
    /// The client code needs initializing data.
    NeedInit,
    /// The client code is ready to calculate the forces.
    Ready,
    /// The client has finished computing the potential and forces.
    HaveData,
}

/// The message sent from server side (application)
#[derive(Debug, Clone)]
pub enum ServerMessage {
    /// Request the status of the client code
    Status,

    /// Send the client code the initialization data followed by an integer
    /// corresponding to the bead index, another integer giving the number of
    /// bits in the initialization string, and finally the initialization string
    /// itself.
    Init(InitData),

    /// Send the client code the cell and cartesion positions.
    PosData(Molecule),

    /// Get the potential and forces computed by client code
    GetForce,

    /// Request to exit
    Exit,
}

/// The message sent by client code (VASP ...)
#[derive(Debug, Clone)]
pub enum ClientMessage {
    ForceReady(Computed),
    Status(ClientStatus),
}

#[derive(Debug, Clone)]
pub struct Computed {
    energy: f64,
    forces: Vec<[f64; 3]>,
    virial: [f64; 9],
    extra: String,
}

impl Computed {
    fn from_model_properties(mp: &gosh::model::ModelProperties) -> Self {
        Self {
            energy: mp.get_energy().unwrap(),
            forces: mp.get_forces().unwrap().clone(),
            // TODO: we have no support for stress tensor, so set virial as
            // zeros
            virial: [0.0; 9],
            extra: "".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitData {
    ibead: usize,
    nbytes: usize,
    init: String,
}

impl InitData {
    fn new(ibead: usize, init: &str) -> Self {
        Self {
            ibead,
            nbytes: init.len(),
            init: init.into(),
        }
    }
}
// base:1 ends here

// [[file:../vasp-tools.note::*pub/as client][pub/as client:1]]
use gosh::model::*;

pub async fn bbm_as_ipi_client(mut bbm: BlackBoxModel, mol_ini: Molecule, sock: &Path) -> Result<()> {
    use futures::SinkExt;
    use futures::StreamExt;
    use tokio::net::UnixStream;
    use tokio_util::codec::{FramedRead, FramedWrite};

    let mut stream = UnixStream::connect(sock).await?;
    let (read, write) = stream.split();

    // the message we received from the server (the driver)
    let mut server_read = FramedRead::new(read, codec::ServerCodec);
    // the message we sent to the server (the driver)
    let mut client_write = FramedWrite::new(write, codec::ClientCodec);

    let mut mol_to_compute: Option<Molecule> = None;
    // NOTE: There is no async for loop for stream in current version of Rust,
    // so we use while loop instead
    while let Some(stream) = server_read.next().await {
        let mut stream = stream?;
        match stream {
            ServerMessage::Status => {
                info!("server ask for client status");
                if mol_to_compute.is_none() {
                    client_write.send(ClientMessage::Status(ClientStatus::Ready)).await?;
                } else {
                    client_write.send(ClientMessage::Status(ClientStatus::HaveData)).await?;
                }
            }
            ServerMessage::GetForce => {
                info!("server ask for forces");
                if let Some(mol) = mol_to_compute.as_mut() {
                    assert_eq!(mol.natoms(), mol_ini.natoms());
                    // NOTE: reset element symbols from mol_ini
                    mol.set_symbols(mol_ini.symbols());
                    let mp = bbm.compute(&mol)?;
                    let computed = Computed::from_model_properties(&mp);
                    client_write.send(ClientMessage::ForceReady(computed)).await?;
                    mol_to_compute = None;
                } else {
                    bail!("not mol to compute!");
                }
            }
            ServerMessage::PosData(mol) => {
                info!("server sent mol {:?}", mol);
                mol_to_compute = Some(mol);
            }
            ServerMessage::Init(data) => {
                info!("server sent init data: {:?}", data);
            }
            ServerMessage::Exit => {
                info!("server ask exit");
                break;
            }
        }
    }

    Ok(())
}
// pub/as client:1 ends here

// [[file:../vasp-tools.note::*pub/as driver][pub/as driver:1]]
async fn ipi_driver(sock: &Path, mol: &Molecule) -> Result<()> {
    use futures::SinkExt;
    use futures::StreamExt;
    use tokio::net::UnixListener;
    use tokio_util::codec::{FramedRead, FramedWrite};

    let mut listener = UnixListener::bind(sock).context("bind unix socket")?;
    let (mut stream, _) = listener.accept().await.context("accept new unix socket client")?;
    let (read, write) = stream.split();
    
    // the message we received from the client code (VASP, SIESTA, ...)
    let mut client_read = FramedRead::new(read, codec::ClientCodec);
    // the message we sent to the client
    let mut server_write = FramedWrite::new(write, codec::ServerCodec);

    loop {
        // ask for client status
        server_write.send(ServerMessage::Status).await?;
        // read the message
        if let Some(stream) = client_read.next().await {
            let stream = stream?;
            match stream {
                // we are ready to send structure to compute
                ClientMessage::Status(status) => match status {
                    ClientStatus::Ready => {
                        server_write.send(ServerMessage::PosData(mol.clone())).await?;
                    }
                    ClientStatus::NeedInit => {
                        let init = InitData::new(0, "");
                        server_write.send(ServerMessage::Init(init)).await?;
                    }
                    ClientStatus::HaveData => {
                        server_write.send(ServerMessage::GetForce).await?;
                    }
                },
                // the computation is done, and we got the results
                ClientMessage::ForceReady(computed) => {
                    dbg!(computed);
                    break;
                }
            }
        }
    }
    Ok(())
}
// pub/as driver:1 ends here

// [[file:../vasp-tools.note::*test2][test2:1]]
#[tokio::test]
async fn test_ipi_driver() -> Result<()> {
    use gosh::gchemol::prelude::*;
    gut::cli::setup_logger_for_test();

    let sock  = "/scratch/.tmpyjc64l/siesta.sock";
    let mol = Molecule::from_file("/share/apps/siesta/scratch/POSCAR4")?;
    ipi_driver(sock.as_ref(), &mol).await?;

    Ok(())
}
// test2:1 ends here
