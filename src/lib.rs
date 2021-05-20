// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use nix::unistd::Pid;
// imports:1 ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod incar;
mod server;
mod socket;
mod task;
mod vasp;
// mods:1 ends here

// [[file:../vasp-tools.note::*pidfile][pidfile:1]]
#[derive(Debug)]
pub struct PidFile {
    file: std::fs::File,
    path: PathBuf,
}

impl PidFile {
    // 生成vasp.pid文件, 如果vasp server已运行, 将报错
    fn create<P: AsRef<Path>>(path: P) -> Result<PidFile> {
        use fs2::*;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .context("Could not create PID file")?;

        // https://docs.rs/fs2/0.4.3/fs2/trait.FileExt.html
        file.try_lock_exclusive()
            .context("Could not lock PID file; Is the daemon already running?")?;

        Ok(PidFile {
            file,
            path: path.as_ref().to_owned(),
        })
    }

    // 写入daemon进程号, 方便外部控制
    fn write_pid(&mut self, pid: Pid) -> Result<()> {
        writeln!(&mut self.file, "{}", pid).context("Could not write PID file")?;
        self.file.flush().context("Could not flush PID file")
    }
}

impl Drop for PidFile {
    // daemon退出时, 清理pidfile
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
// pidfile:1 ends here

// [[file:../vasp-tools.note::*daemon][daemon:1]]
#[cfg(unix)]
pub fn daemonize() -> Result<PidFile> {
    // use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixListener;

    // https://man7.org/linux/man-pages/man3/daemon.3.html
    // 根据Linux API手册, 这里不执行double fork, 因此该daemon是session leader
    // the resulting daemon is a session leader
    nix::unistd::daemon(true, true)?;

    // 生成进程session, 方便退出时处理子进程
    // if nix::unistd::setsid().is_err() {
    //     eprintln!("Could not create new session");
    // }
    let sid = std::process::id() as i32;
    println!(" my pid {}", sid);

    let mut pid_file = PidFile::create("vasp-server.pid")?;
    pid_file.write_pid(Pid::from_raw(sid))?;

    let console_sock: PathBuf = "vasp-server.sock".into();

    // https://stackoverflow.com/questions/40218416/how-do-i-close-a-unix-socket-in-rust
    // servers should unlink the socket pathname prior to binding it.
    let listener = UnixListener::bind(&console_sock)?;
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut msg = String::new();
        let n = stream.read_to_string(&mut msg)?;
        dbg!(n);
        let action = msg.trim();
        if dbg!(action) == "exit" {
            break;
        }
    }

    Ok(pid_file)
}
// daemon:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use crate::task::*;

// FIXME: remove
pub use crate::server::*;
pub use crate::socket::*;

pub mod adhoc {
    pub use crate::vasp::*;
}
// pub:1 ends here
