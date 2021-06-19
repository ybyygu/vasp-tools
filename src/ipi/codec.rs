// [[file:../../vasp-tools.note::*imports][imports:1]]
use super::*;

use bytes::{Buf, BufMut};
use bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

type EncodedResult = Result<(), std::io::Error>;

const HEADER_SIZE: usize = 12;
const Bohr: f64 = 0.5291772105638411;
const Hatree: f64 = 27.211386024367243;
// imports:1 ends here

// [[file:../../vasp-tools.note::*utils][utils:1]]
// A wrapper for Ok(None), so we can early return using question mark (?)
#[derive(Debug)]
enum DecodeError {
    IoError(std::io::Error),
    // but a frame isn’t fully available yet, then Ok(None) is returned
    NotEnoughData,
}

fn fix_decode_err<T>(e: DecodeError) -> Result<Option<T>, std::io::Error> {
    match e {
        DecodeError::IoError(e) => Err(e),
        DecodeError::NotEnoughData => Ok(None),
    }
}

fn try_to_string(bytes: &[u8]) -> Result<String, std::io::Error> {
    let bytes: Bytes = bytes.into_iter().cloned().collect();
    String::from_utf8(bytes.to_vec()).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

fn to_u32(bytes: &[u8]) -> u32 {
    assert_eq!(bytes.len(), 4);
    let mut bytes: Bytes = bytes.into_iter().cloned().collect();
    bytes.get_u32_le()
}

/// Try to decode message header
fn try_decode_message_header(src: &BytesMut, nheader: usize) -> Result<String, DecodeError> {
    if src.len() < nheader {
        return Err(DecodeError::NotEnoughData);
    }

    let s = try_to_string(&src[..nheader]).map_err(|e| into_decode_error(e))?;
    Ok(s.trim_end().to_string())
}

/// Try to decode length header
fn try_decode_length_header_u32(src: &BytesMut, offset: usize) -> Result<usize, DecodeError> {
    let nheader = offset + 4;
    if src.len() < nheader {
        return Err(DecodeError::NotEnoughData);
    }
    let n = to_u32(&src[offset..nheader]) as usize;
    if src.len() < nheader + n {
        return Err(DecodeError::NotEnoughData);
    }

    Ok(n)
}

/// Try to read in n bytes
fn try_decode_nbytes(src: &BytesMut, nbytes: usize) -> Result<(), DecodeError> {
    if src.len() < nbytes {
        Err(DecodeError::NotEnoughData)
    } else {
        Ok(())
    }
}

fn into_decode_error(e: std::io::Error) -> DecodeError {
    DecodeError::IoError(e)
}

fn format_header(code: &str) -> String {
    let code = format!("{:12}", code);
    assert_eq!(code.len(), 12);
    code
}

// Encode simple header str
fn encode_header(dest: &mut BytesMut, header: &str) -> EncodedResult {
    assert!(header.len() <= 12);
    dest.put_slice(format_header(header).as_bytes());

    Ok(())
}
// utils:1 ends here

// [[file:../../vasp-tools.note::*client/status][client/status:1]]
fn encode_client_status(dest: &mut BytesMut, status: &ClientStatus) -> EncodedResult {
    let s = match status {
        ClientStatus::NeedInit => "NEEDINIT",
        ClientStatus::Ready => "READY",
        ClientStatus::HaveData => "HAVEDATA",
    };
    encode_header(dest, s)?;

    Ok(())
}

fn decode_client_status(src: &BytesMut) -> Result<ClientStatus, DecodeError> {
    let msg = try_decode_message_header(src, 12)?;
    let status = match msg.as_str() {
        "NEEDINT" => ClientStatus::NeedInit,
        "READY" => ClientStatus::Ready,
        "HAVEDATA" => ClientStatus::HaveData,
        _ => panic!("invalid message: {:?}", msg),
    };
    Ok(status)
}

#[test]
fn test_ipi_status() {
    let mut dest = BytesMut::new();

    let s = ClientStatus::Ready;
    encode_client_status(&mut dest, &s);
    let decoded = decode_client_status(&dest).unwrap();
    assert_eq!(decoded, s);
}
// client/status:1 ends here

// [[file:../../vasp-tools.note::*server/init][server/init:1]]
/// Init Message
/// [12] [4]    [4(?)] [s...]
/// INIT ibead  nbytes  ...
fn decode_init(src: &mut BytesMut) -> Result<InitData, DecodeError> {
    let msg = try_decode_message_header(src, 12)?;
    assert_eq!(msg, "INIT");
    let nbytes = try_decode_length_header_u32(src, 12 + 4)?;
    let n_expected = 12 + 4 + 4 + nbytes;

    src.advance(12);
    let ibead = src.get_u32_le();
    let nbytes = src.get_u32_le();
    let init = src.copy_to_bytes(nbytes as usize);
    let init = try_to_string(&init).map_err(|e| into_decode_error(e))?;
    Ok(InitData::new(0, &init))
}

fn encode_init(dest: &mut BytesMut, init: InitData) -> EncodedResult {
    encode_header(dest, "INIT")?;

    let InitData { ibead, nbytes, init } = init;
    dest.put_u32_le(ibead as u32);
    dest.put_u32_le(nbytes as u32);
    dest.put_slice(init.as_bytes());

    Ok(())
}

#[test]
fn test_ipi_init() {
    let mut dest = BytesMut::new();
    encode_init(&mut dest, InitData::new(0, "XX")).unwrap();
    let x = decode_init(&mut dest).unwrap();
    assert_eq!(x.init, "XX");
}
// server/init:1 ends here

// [[file:../../vasp-tools.note::*server/start compute][server/start compute:2]]
use gosh::gchemol::{Atom, Lattice, Molecule};
use vecfx::*;

fn is_periodic(cell: [f64; 9]) -> bool {
    cell.into_iter().map(|x| x.abs()).sum::<f64>() > 1e-6
}

fn decode_posdata(src: &mut BytesMut) -> Result<Molecule, DecodeError> {
    // 0. try to decode no advance, until we have enough data
    let msg = try_decode_message_header(src, 12)?;
    assert_eq!(msg, "POSDATA");

    let nbytes_cell = 9 * 8 * 2; // cell matrix and the inverse of cell matrix
    let nbytes_expected = 12 + nbytes_cell;
    let natoms = try_decode_length_header_u32(&src, nbytes_expected)?;

    let nbytes_cart_coords = 3 * 8 * natoms;
    let nbytes_expected = nbytes_expected + 4 + nbytes_cart_coords;
    try_decode_nbytes(src, nbytes_expected)?;

    // 1. start read message
    src.advance(12);
    let mut cell = [0f64; 9];
    // nine floats for the cell vector matrix
    for i in 0..9 {
        cell[i] = src.get_f64_le() * Bohr;
    }

    // read inverse matrix of the cell
    // NOTE: we do not need this actually
    // nine floats for the inverse matrix
    let mut _icell = [0f64; 9];
    for i in 0..9 {
        _icell[i] = src.get_f64_le() * Bohr;
    }

    let natoms = src.get_u32_le() as usize;
    let mut coords = vec![[0f64; 3]; natoms];
    for i in 0..natoms {
        let x = src.get_f64_le() * Bohr;
        let y = src.get_f64_le() * Bohr;
        let z = src.get_f64_le() * Bohr;
        coords[i] = [x, y, z];
    }

    // FIXME: how to determinate element symbols?
    let atoms: Vec<_> = coords.into_iter().map(|p| Atom::new("C", p)).collect();
    let mut mol = Molecule::from_atoms(atoms);

    // NOTE: The cell is transposed when transfering
    if is_periodic(cell) {
        let mat = Matrix3f::from_row_slice(&cell);
        let lat = Lattice::from_matrix(mat);
        mol.set_lattice(lat);
    } else {
        debug!("i-pi: non-periodic system");
    }

    Ok(mol)
}

fn encode_posdata(dest: &mut BytesMut, mol: &Molecule) -> EncodedResult {
    encode_header(dest, "POSDATA")?;

    let (cell, icell) = mol.get_lattice().as_ref().map_or_else(
        // NOTE: for non-periodic system, we use a cell in zero size
        || (Matrix3f::zeros(), Matrix3f::zeros()),
        |lat| (lat.matrix(), lat.inv_matrix()),
    );

    // I-PI assumes row major order for cell matrix
    for v in cell.transpose().as_slice() {
        dest.put_f64_le(*v / Bohr);
    }
    // I-PI assumes row major order for cell matrix
    for v in icell.transpose().as_slice() {
        dest.put_f64_le(*v * Bohr);
    }

    // write Cartesian coordinates
    dest.put_u32_le(mol.natoms() as u32);
    for [x, y, z] in mol.positions() {
        dest.put_f64_le(x / Bohr);
        dest.put_f64_le(y / Bohr);
        dest.put_f64_le(z / Bohr);
    }

    Ok(())
}

#[test]
fn test_decode_posdata() {
    use approx::*;
    use gosh::gchemol::prelude::*;

    let mol1 = Molecule::from_file("tests/files/quinone.cif").unwrap();
    let mut dest = BytesMut::new();
    encode_posdata(&mut dest, &mol1);
    let mol2 = decode_posdata(&mut dest).unwrap();
    assert_eq!(mol1.natoms(), mol2.natoms());
    let [va1, vb1, vc1] = mol1.get_lattice().unwrap().vectors();
    let [va2, vb2, vc2] = mol2.get_lattice().unwrap().vectors();
    for i in 0..3 {
        assert_relative_eq!(va1[i], va2[i], epsilon = 1e-4);
        assert_relative_eq!(vb1[i], vb2[i], epsilon = 1e-4);
        assert_relative_eq!(vc1[i], vc2[i], epsilon = 1e-4);
    }
    let p1: Vec<_> = mol1.positions().collect();
    let p2: Vec<_> = mol2.positions().collect();
    for i in 0..p1.len() {
        for j in 0..3 {
            assert_relative_eq!(p1[i][j], p2[i][j], epsilon = 1e-4);
        }
    }
}
// server/start compute:2 ends here

// [[file:../../vasp-tools.note::*client/compute done][client/compute done:1]]
fn encode_client_computed(dst: &mut BytesMut, computed: &Computed) -> EncodedResult {
    let s = format_header("FORCEREADY");
    dst.put_slice(s.as_bytes());
    dst.put_f64_le(computed.energy / Hatree);
    let n = computed.forces.len();
    dst.put_u32_le(n as u32);
    let f = Bohr / Hatree;
    for i in 0..n {
        dst.put_f64_le(computed.forces[i][0] * f);
        dst.put_f64_le(computed.forces[i][1] * f);
        dst.put_f64_le(computed.forces[i][2] * f);
    }
    for i in 0..9 {
        dst.put_f64_le(computed.virial[i] * Hatree);
    }
    let n = computed.extra.len();
    dst.put_u32_le(n as u32);
    dst.put_slice(computed.extra.as_bytes());

    Ok(())
}

fn decode_client_computed(src: &mut BytesMut) -> Result<Computed, DecodeError> {
    let nheader = 12;
    let msg = try_decode_message_header(src, nheader)?;
    assert_eq!(msg, "FORCEREADY");

    // try to read natoms
    let nenergy = 8;
    let natoms = try_decode_length_header_u32(src, nheader + nenergy)?;
    let nforces = 3 * natoms * 8;
    let nviral = 9 * 8; // nine float numbers (f64)
    let nbytes_expected = 12 + 8 + 4 + nforces + nviral;
    // try to read extra data
    let nextra = try_decode_length_header_u32(src, nbytes_expected)?;

    // start reading message now
    src.advance(nheader);
    let energy = src.get_f64_le() * Hatree;
    let natoms = src.get_u32_le() as usize;
    let mut forces = vec![[0.0; 3]; natoms];
    for i in 0..natoms {
        for j in 0..3 {
            forces[i][j] = src.get_f64_le() * Hatree / Bohr;
        }
    }
    let mut virial = [0.0; 9];
    for i in 0..9 {
        virial[i] = src.get_f64_le() * Hatree;
    }
    let nextra = src.get_u32_le();
    let bytes = src.copy_to_bytes(nextra as usize);
    let extra = try_to_string(&bytes).map_err(|e| into_decode_error(e))?;

    let computed = Computed {
        energy,
        forces,
        extra,
        virial,
    };

    Ok(computed.into())
}
// client/compute done:1 ends here

// [[file:../../vasp-tools.note::*pub/client][pub/client:1]]
pub struct ClientCodec;

impl Decoder for ClientCodec {
    type Item = ClientMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match try_decode_message_header(src, 12) {
            Ok(header_str) => match header_str.as_str() {
                "NEEDINIT" => {
                    src.advance(12);
                    Ok(Some(ClientMessage::Status(ClientStatus::NeedInit)))
                }
                "READY" => {
                    src.advance(12);
                    Ok(Some(ClientMessage::Status(ClientStatus::Ready)))
                }
                "HAVADATA" => {
                    src.advance(12);
                    Ok(Some(ClientMessage::Status(ClientStatus::HaveData)))
                }
                "FORCEREADY" => match decode_client_computed(src) {
                    Err(e) => fix_decode_err(e),
                    Ok(computed) => Ok(Some(ClientMessage::ForceReady(computed))),
                },
                _ => {
                    error!("invalid header: {}", header_str);
                    todo!();
                }
            },
            Err(e) => fix_decode_err(e),
        }
    }
}

impl Encoder<ClientMessage> for ClientCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: ClientMessage, dest: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            ClientMessage::Status(status) => encode_client_status(dest, &status),
            ClientMessage::ForceReady(computed) => encode_client_computed(dest, &computed),
        }
    }
}
// pub/client:1 ends here

// [[file:../../vasp-tools.note::*pub/server][pub/server:1]]
pub struct ServerCodec;
impl Decoder for ServerCodec {
    type Item = ServerMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match try_decode_message_header(src, 12) {
            Ok(header_str) => match header_str.as_str() {
                "STATUS" => {
                    src.advance(12);
                    Ok(Some(ServerMessage::Status))
                }
                "GETFORCE" => {
                    src.advance(12);
                    Ok(Some(ServerMessage::GetForce))
                }
                "EXIT" => {
                    src.advance(12);
                    Ok(Some(ServerMessage::Exit))
                }
                "INIT" => match decode_init(src) {
                    Err(e) => fix_decode_err(e),
                    Ok(init_data) => Ok(Some(ServerMessage::Init(init_data))),
                },
                "POSDATA" => match decode_posdata(src) {
                    Err(e) => fix_decode_err(e),
                    Ok(mol) => Ok(Some(ServerMessage::PosData(mol))),
                },
                _ => {
                    error!("invalid header: {}", header_str);
                    todo!();
                }
            },
            Err(e) => fix_decode_err(e),
        }
    }
}

impl Encoder<ServerMessage> for ServerCodec {
    type Error = std::io::Error;

    fn encode(&mut self, msg: ServerMessage, dest: &mut BytesMut) -> Result<(), Self::Error> {
        match msg {
            ServerMessage::Status => encode_header(dest, "STATUS"),
            ServerMessage::GetForce => encode_header(dest, "GETFORCE"),
            ServerMessage::Exit => encode_header(dest, "EXIT"),
            ServerMessage::Init(data) => encode_init(dest, data),
            ServerMessage::PosData(mol) => encode_posdata(dest, &mol),
        }
    }
}
// pub/server:1 ends here
