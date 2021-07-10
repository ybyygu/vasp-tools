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
        let energy = dbg!(mp.get_energy().unwrap());
        let forces = mp.get_forces().unwrap().clone();
        Self {
            energy,
            forces,
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

    // FIXME: temp solution: write flame yaml input
    let [va, vb, vc] = mol_ini.get_lattice().as_ref().unwrap().vectors();
    println!("---");
    println!("conf:");
    println!("  bc: slab");
    println!("  nat: {}", mol_ini.natoms());
    println!("  units_length: angstrom");
    println!("  cell:");
    println!("  - [{:10.4}, {:10.4}, {:10.4}]", va[0], va[1], va[2]);
    println!("  - [{:10.4}, {:10.4}, {:10.4}]", vb[0], vb[1], vb[2]);
    println!("  - [{:10.4}, {:10.4}, {:10.4}]", vc[0], vc[1], vc[2]);
    println!("  coord:");
    for (i, a) in mol_ini.atoms() {
        let [x, y, z] = a.position();
        let fff: String = a.freezing().iter().map(|&x| if x { "T" } else { "F" }).collect();
        println!("  - [{:10.4}, {:10.4}, {:10.4}, {}, {}]", x, y, z, a.symbol(), fff);
    }

    // let mut stream = UnixStream::connect(sock).context("connect to unix socket").await?;
    let mut stream = tokio::net::TcpStream::connect("127.0.0.1:10244")
        .await
        .context("connect to host")?;
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
                debug!("server ask for client status");
                if mol_to_compute.is_none() {
                    client_write.send(ClientMessage::Status(ClientStatus::Ready)).await?;
                } else {
                    client_write.send(ClientMessage::Status(ClientStatus::HaveData)).await?;
                }
            }
            ServerMessage::GetForce => {
                debug!("server ask for forces");
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
                debug!("server sent mol {:?}", mol);
                mol_to_compute = Some(mol);
            }
            ServerMessage::Init(data) => {
                debug!("server sent init data: {:?}", data);
            }
            ServerMessage::Exit => {
                debug!("server ask exit");
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
