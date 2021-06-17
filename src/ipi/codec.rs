// [[file:../../vasp-tools.note::*imports][imports:1]]
use super::*;

use bytes::Buf;
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

const HEADER_SIZE: usize = 12;

fn invalid_data_error(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

fn to_string(bytes: BytesMut) -> Result<String, std::io::Error> {
    String::from_utf8(bytes.to_vec())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}
// imports:1 ends here

// [[file:../../vasp-tools.note::*client][client:1]]
pub struct ClientCodec {}

impl Decoder for ClientCodec {
    type Item = ClientMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        todo!();
    }
}
impl Encoder<ClientMessage> for ClientCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: ClientMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        todo!();
    }
}
// client:1 ends here

// [[file:../../vasp-tools.note::*server][server:1]]
pub struct ServerCodec;
impl Decoder for ServerCodec {
    type Item = ServerMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // but a frame isn’t fully available yet, then Ok(None) is returned
        if src.len() < HEADER_SIZE {
            return Ok(None);
        }

        let mut header_bytes = [0u8; HEADER_SIZE];
        header_bytes.copy_from_slice(&src[..HEADER_SIZE]);
        let header_str = String::from_utf8_lossy(&header_bytes);

        match header_str.trim_end() {
            "STATUS" => {
                // we are ready. Use advance to modify src such that it no
                // longer contains this frame.
                src.advance(12);
                Ok(Some(ServerMessage::Status))
            }
            "INIT" => {
                // 4 bytes + 4 bytes + nbytes
                if src.len() < 12 + 4 + 4 {
                    return Ok(None);
                }
                // read init string
                let mut n1 = [0u8; 4];
                n1.copy_from_slice(&src[12..12 + 4]);
                let mut n2 = [0u8; 4];
                n2.copy_from_slice(&src[12 + 4..12 + 4 + 4]);
                let ibead = u32::from_le_bytes(n1);
                let nbytes = u32::from_le_bytes(n2);

                if src.len() < 12 + 4 + 4 + nbytes {
                    return Ok(None);
                }
                let mut n = vec![0u8; nbytes];
                n.copy_from_slice(&src[12 + 4 + 4..12 + 4 + 4 + nbytes]);
                let init = String::from_utf8_lossy(&n).to_string();

                src.advance(12 + 4 + 4 + nbytes);
                Ok(Some(ServerMessage::Init { ibead, nbytes, init }))
            }
            "POSDATA" => {
                //
                Ok(Some(ServerMessage::PosData))
            }
            "GETFORCE" => {
                //
                Ok(Some(ServerMessage::GetForce))
            }
            "EXIT" => {
                //
                Ok(Some(ServerMessage::Exit))
            }
            _ => {
                error!("invalid header: {}", header_str);
                let e = invalid_data_error(&header_str);
                Err(e)
            }
        }
    }
}

impl Encoder<ServerMessage> for ServerCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: ServerMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        todo!();
    }
}
// server:1 ends here
