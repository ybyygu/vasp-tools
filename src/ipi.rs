// [[file:../vasp-tools.note::*imports][imports:1]]
use crate::common::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod codec;
// mods:1 ends here

// [[file:../vasp-tools.note::*base][base:1]]
/// The Message type sent from client side (the computation engine)
pub enum ClientStatus {
    /// The client code needs initializing data.
    NeedInit,
    /// The client code is ready to calculate the forces.
    Ready,
    /// The client has finished computing the potential and forces.
    HaveData,
}

/// The message sent from server side (application)
pub enum ServerMessage {
    /// Request the status of the client code
    Status,

    /// Send the client code the initialization data followed by an integer
    /// corresponding to the bead index, another integer giving the number of
    /// bits in the initialization string, and finally the initialization string
    /// itself.
    Init { ibead: u32, nbytes: u32, init: String },
    /// Send the client code the cell and cartesion positions.
    PosData,
    /// Get the potential and forces computed by client code
    GetForce,
    /// Request to exit
    Exit,
}

/// The message sent by client code (VASP ...)
pub enum ClientMessage {
    NeedInt,
    ForceReady,
    ClientStatus,
}
// base:1 ends here
