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
