// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use nix::unistd::Pid;
// imports:1 ends here

// [[file:../vasp-server.note::*mods][mods:1]]
mod vasp;
// mods:1 ends here

// [[file:../vasp-server.note::*pidfile][pidfile:1]]
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

// [[file:../vasp-server.note::*daemon][daemon:1]]
#[cfg(unix)]
pub fn daemonize() -> Result<PidFile> {
    // use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixListener;

    // https://man7.org/linux/man-pages/man3/daemon.3.html
    // 根据Linux API手册, 这里不执行double fork, 因此该daemon是session leader
    // the resulting daemon is a session leader
    nix::unistd::daemon(true, true)?;

    // 生成进程session, 方便退出时处理子进程
    if nix::unistd::setsid().is_err() {
        eprintln!("Could not create new session");
    }
    let sid = std::process::id() as i32;
    println!(" my pid {}", sid);
    let mut pid_file = PidFile::create("vasp-server.pid")?;
    pid_file.write_pid(Pid::from_raw(sid))?;

    let mut count = 0u32;
    loop {
        count += 1;
        print!("{} ", count);
        if count == 60 {
            println!("OK, that's enough");
            // Exit this loop
            break;
        }
        std::thread::sleep(std::time::Duration::new(1, 0));
    }

    let console_sock: PathBuf = "vasp-server.sock".into();

    // https://stackoverflow.com/questions/40218416/how-do-i-close-a-unix-socket-in-rust
    // servers should unlink the socket pathname prior to binding it.
    let listener = UnixListener::bind(&console_sock)?;
    let (mut stream, _sockaddr) = listener.accept()?;
    // let stream_fd = stream.as_raw_fd();

    // // 从socket文件中读
    // let mut buf = [0 as u8; 4096];
    // stream.read_exact(&mut buf)?;

    // // 写入socket文件
    // stream.write_all(&buf)?;

    Ok(pid_file)
}
// daemon:1 ends here