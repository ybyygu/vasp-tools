// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use gchemol::prelude::*;
use gchemol::Molecule;
use gosh::gchemol;

use rexpect::reader::{NBReader, ReadUntil};

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-server.note::*base][base:1]]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use std::io::prelude::*;
use std::io::BufReader;
use std::io::LineWriter;

pub(crate) struct Task {
    child: Child,
    // stdin
    // stream0: UnixStream,
    stream0: ChildStdin,
    // stdou
    // stream1: std::io::Lines<BufReader<UnixStream>>,
    stream1: NBReader,
}

impl Task {
    pub fn new(
        mut child: Child,
        // stream: UnixStream
    ) -> Self {
        // let stream1 = stream.try_clone().unwrap();
        let stream0 = child.stdin.take().unwrap();
        let stream1 = child.stdout.take().unwrap();
        Self {
            stream0,
            // stream1: BufReader::new(stream1).lines(),
            stream1: NBReader::new(stream1, None),
            child,
        }
    }
}
// base:1 ends here

// [[file:../vasp-server.note::*stop][stop:1]]
impl Drop for Task {
    fn drop(&mut self) {
        // if let Err(msg) = crate::vasp::write_stopcar() {
        //     eprintln!("Failed to stop vasp server: {:?}", msg);
        // }
    }
}
// stop:1 ends here

// [[file:../vasp-server.note::*read][read:1]]
use std::sync::{Arc, Mutex};

/// Pipe streams are blocking, we need separate threads to monitor them without blocking the primary thread.
fn child_stream_to_vec<R>(mut stream: R) -> Arc<Mutex<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    let out = Arc::new(Mutex::new(Vec::new()));
    let vec = out.clone();
    std::thread::Builder::new()
        .name("child_stream_to_vec".into())
        .spawn(move || loop {
            let mut buf = [0];
            match stream.read(&mut buf) {
                Err(err) => {
                    println!("{}] Error reading from stream: {}", line!(), err);
                    break;
                }
                Ok(got) => {
                    if got == 0 {
                        break;
                    } else if got == 1 {
                        vec.lock().expect("!lock").push(buf[0])
                    } else {
                        println!("{}] Unexpected number of bytes: {}", line!(), got);
                        break;
                    }
                }
            }
        })
        .expect("!thread");
    out
}
// read:1 ends here

// [[file:../vasp-server.note::*input][input:1]]
impl Task {
    /// write scaled positions to VASP stdin
    pub fn input_positions(&mut self, mol: &Molecule) -> Result<()> {
        let mut lines = mol
            .get_scaled_positions()
            .expect("lattice")
            .map(|[x, y, z]| format!("{:19.16} {:19.16} {:19.16}\n", x, y, z));

        for line in lines {
            self.stream0.write_all(line.as_bytes())?;
        }
        self.stream0.flush()?;

        Ok(())
    }
}
// input:1 ends here

// [[file:../vasp-server.note::*compute & output][compute & output:1]]
use gosh::model::ModelProperties;

impl Task {
    pub fn compute_mol(&mut self, mol: &Molecule) -> Result<ModelProperties> {
        log_dbg!();

        let (txt, _) = self
            .stream1
            .read_until(&ReadUntil::String("POSITIONS: reading from stdin\n".to_string()))
            .unwrap();

        let (energy, forces) = crate::vasp::stdout::parse_energy_and_forces(&txt)?;
        let mut mp = ModelProperties::default();
        mp.set_energy(energy);
        mp.set_forces(forces);
        return Ok(mp);
    }
}
// compute & output:1 ends here
