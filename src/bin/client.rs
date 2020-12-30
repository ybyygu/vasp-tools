// [[file:../../vasp-server.note::*bin/client.rs][bin/client.rs:1]]
use gut::prelude::*;

use std::io::prelude::*;
use std::os::unix::net::UnixStream;

fn main() -> Result<()> {
    let addr = "/tmp/vasp-server.sock";

    // Connect to socket
    let mut stream = UnixStream::connect(&addr).context("connect to socket server")?;

    // Send message
    let msg = "exit";
    stream.write(msg.as_bytes())?;

    Ok(())
}
// bin/client.rs:1 ends here
