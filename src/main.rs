// [[file:../vasp-server.note::*main.rs][main.rs:1]]
#[cfg(unix)]
fn main() {
    use gut::prelude::*;

    let daemon = vasp_server::daemonize().unwrap();
    dbg!(daemon);
}
// main.rs:1 ends here
