// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;
use std::path::{Path, PathBuf};

use std::io::prelude::*;
// imports:1 ends here

// [[file:../vasp-tools.note::*process group][process group:1]]
macro_rules! setsid {
    () => {{
        // Don't check the error of setsid because it fails if we're the
        // process leader already. We just forked so it shouldn't return
        // error, but ignore it anyway.
        nix::unistd::setsid().ok();

        Ok(())
    }};
}

pub trait ProcessGroupExt<T> {
    fn new_process_group(&mut self) -> &mut T;
}

use std::process::Command;
impl ProcessGroupExt<Command> for Command {
    fn new_process_group(&mut self) -> &mut Command {
        use std::os::unix::process::CommandExt;

        unsafe {
            self.pre_exec(|| setsid!());
        }
        self
    }
}

impl ProcessGroupExt<tokio::process::Command> for tokio::process::Command {
    fn new_process_group(&mut self) -> &mut tokio::process::Command {
        unsafe {
            self.pre_exec(|| setsid!());
        }
        self
    }
}
// process group:1 ends here

// [[file:../vasp-tools.note::*pidfile][pidfile:1]]
use nix::unistd::Pid;

#[derive(Debug)]
pub struct PidFile {
    file: std::fs::File,
    path: PathBuf,
}

impl PidFile {
    fn create(path: &Path) -> Result<PidFile> {
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
            path: path.to_owned(),
        })
    }

    fn write_pid(&mut self, pid: Pid) -> Result<()> {
        writeln!(&mut self.file, "{}", pid).context("Could not write PID file")?;
        self.file.flush().context("Could not flush PID file")
    }

    /// Create a pidfile for process `pid`
    pub fn new(path: &Path, pid: u32) -> Result<Self> {
        let mut pidfile = Self::create(path)?;
        pidfile.write_pid(Pid::from_raw(pid as i32));

        Ok(pidfile)
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
// pidfile:1 ends here

// [[file:../vasp-tools.note::*process handler][process handler:1]]
use shared_child::SharedChild;

use shared_child::unix::SharedChildExt;
// use futures::channel::oneshot::channel;
// use std::time::Duration;

#[derive(Clone)]
pub struct ProcessHandle {
    process: std::sync::Arc<SharedChild>,
}

impl ProcessHandle {
    pub fn new(mut command: &mut Command) -> Result<ProcessHandle> {
        Ok(ProcessHandle {
            process: std::sync::Arc::new(SharedChild::spawn(&mut command)?),
        })
    }

    /// Kill child process
    pub fn kill(&self) {
        let _ = self.process.kill();
    }

    /// Return child process id
    pub fn pid(&self) -> u32 {
        self.process.id()
    }

    /// Check if child process still running
    pub fn check_if_running(&self) -> Result<()> {
        let pid = self.pid();

        let status = self
            .process
            .try_wait()
            .with_context(|| format!("Failed to wait for process {:?}", pid))?;
        let _ = status.ok_or(format_err!("Process [pid={}] is still running.", pid))?;

        Ok(())
    }

    pub fn terminate(&self) -> Result<()> {
        self.process.send_signal(libc::SIGTERM)?;
        // Error means, that probably process was already terminated, because:
        // - We have permissions to send signal, since we created this process.
        // - We specified correct signal SIGTERM.
        // But better let's check.
        self.check_if_running()?;

        Ok(())
    }

    pub fn wait_until_finished(self) -> Result<()> {
        // On another thread, wait on the child process.
        let child_arc_clone = self.process.clone();
        let thread = std::thread::spawn(move || child_arc_clone.wait().unwrap());

        // let process = self.process.clone();
        // let (sender, receiver) = channel::<ExeUnitExitStatus>();

        // std::thread::spawn(move || {
        //     let result = process.wait();

        //     let status = match result {
        //         Ok(status) => match status.code() {
        //             // status.code() will return None in case of termination by signal.
        //             None => ExeUnitExitStatus::Aborted(status),
        //             Some(_code) => ExeUnitExitStatus::Finished(status),
        //         },
        //         Err(error) => ExeUnitExitStatus::Error(error),
        //     };
        //     sender.send(status)
        // });

        // Note: unwrap can't fail here. All sender, receiver and thread will
        // end their lifetime before await will return. There's no danger
        // that one of them will be dropped earlier.
        // return receiver.await.unwrap();

        Ok(())
    }
}
// process handler:1 ends here

// [[file:../vasp-tools.note::*stdout][stdout:2]]
use tokio::io;
use tokio::process::{ChildStdin, ChildStdout};

type ReadOutput = tokio::sync::mpsc::Receiver<String>;
type ReadOutputTx = tokio::sync::mpsc::Sender<String>;
type WriteInput = tokio::sync::mpsc::Receiver<String>;
type WriteInputTx = tokio::sync::mpsc::Sender<String>;
// The part of stdout read-in
type ReadInOutput = tokio::sync::watch::Receiver<String>;
type ReadInOutputTx = tokio::sync::watch::Sender<String>;

pub struct StdoutReader {
    // reader: tokio::io::Lines<io::BufReader<ChildStdout>>,
    // reader: ChildStdout,
    reader: io::BufReader<ChildStdout>,
}

impl StdoutReader {
    pub fn new(stdout: ChildStdout) -> Self {
        use io::AsyncBufReadExt;

        // let reader = io::BufReader::new(stdout).lines();
        let reader = io::BufReader::new(stdout);
        Self { reader }
    }

    /// Read stdout until finding a line containing the `pattern`
    pub async fn read_until(&mut self, pattern: &str) -> Result<String> {
        use io::AsyncBufReadExt;

        info!("read stdout until finding pattern: {:?}", pattern);
        // let mut text = String::new();
        // while let Some(line) = self.reader.next_line().await? {
        //     info!("read in line: {:?}", line);
        //     writeln!(&mut text, "{}", line)?;
        //     if line.contains(&pattern) {
        //         info!("found pattern: {:?}", pattern);
        //         return Ok(text);
        //     }
        // }
        let mut text = String::new();
        loop {
            dbg!("xxidir");
            let size = self.reader.read_line(&mut text).await?;
            dbg!("xxidir");
            if size == 0 {
                break;
            }
            return Ok(text);
        }
        bail!("expected pattern not found!");
    }
}

mod stdout {
    use gut::prelude::*;
    use std::io::{self, prelude::*};
    use std::process::ChildStdout;

    pub struct StdoutReader {
        reader: io::Lines<io::BufReader<ChildStdout>>,
    }

    impl StdoutReader {
        pub fn new(stdout: ChildStdout) -> Self {
            let reader = io::BufReader::new(stdout).lines();
            Self { reader }
        }

        /// Read stdout until finding a line containing the `pattern`
        pub fn read_until(&mut self, pattern: &str) -> Result<String> {
            info!("read stdout until finding pattern: {:?}", pattern);
            let mut text = String::new();
            while let Some(line) = self.reader.next() {
                let line = line.context("invalid encoding?")?;
                info!("read in line: {:?}", line);
                writeln!(&mut text, "{}", line)?;
                if line.contains(&pattern) {
                    info!("found pattern: {:?}", pattern);
                    return Ok(text);
                }
            }
            bail!("expected pattern not found!");
        }
    }
}
// stdout:2 ends here

// [[file:../vasp-tools.note::*output][output:1]]
mod reader {
    use gut::prelude::*;
    use std::io::{self, prelude::*};
    use std::process::ChildStdout;

    // The part of stdout read-in
    type ReadInOutput = tokio::sync::watch::Receiver<String>;
    type ReadInOutputTx = tokio::sync::watch::Sender<String>;

    pub struct StdoutReader {
        stdout: Option<ChildStdout>,
        rx: Option<ReadInOutput>,
        tx: Option<ReadInOutputTx>,
    }

    impl StdoutReader {
        pub fn new(stdout: ChildStdout) -> Self {
            info!("new stdout reader");
            let (tx, rx) = tokio::sync::watch::channel("".to_string());
            Self {
                stdout: stdout.into(),
                tx: tx.into(),
                rx: rx.into(),
            }
        }

        /// Read stdout until finding a line containing the `pattern`
        pub async fn read_until(&mut self, pattern: &str) -> Result<String> {
            info!("read stdout until finding pattern: {:?}", pattern);
            let mut stdout = self.stdout.take().unwrap();

            // spawn the task for read stdout only once
            if let Some(tx) = self.tx.take() {
                read_child_stream_until(stdout, pattern.into(), tx).await?;
            }
            let mut rx = self.rx.clone().unwrap();
            // return the output already read in from child process's stdout
            tokio::spawn(async move {
                if rx.changed().await.is_ok() {
                    let out = rx.borrow().to_string();
                    debug!("read in stdout {:?} bytes", out.len());
                    dbg!(out);
                }
            });
            Ok("test".into())
        }
    }

    // https://stackoverflow.com/a/34616729
    /// Pipe streams are blocking, we need separate threads to monitor them without blocking the primary thread.
    async fn read_child_stream_until<R: Read + Send + 'static>(
        mut stream: R,
        pattern: String,
        tx: ReadInOutputTx,
    ) -> Result<()> {
        tokio::spawn(async move {
            info!("read stream for pattern: {}", pattern);
            let mut out = vec![];
            loop {
                debug!("read in data byte by byte");
                let mut buf = [0];
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(1) => out.push(dbg!(buf[0])),
                    Err(e) => {
                        break;
                        dbg!(e);
                    }
                    _ => todo!(),
                }
                let msg = String::from_utf8_lossy(&out);
                if msg.contains(&pattern) {
                    tx.send(msg.to_string()).unwrap();
                }
            }
        });

        Ok(())
    }

    use std::sync::{Arc, Mutex};
    type SharedData = Arc<Mutex<Vec<u8>>>;
    // https://stackoverflow.com/a/34616729
    /// Pipe streams are blocking, we need separate threads to monitor them without blocking the primary thread.
    fn read_child_stream_<R: Read + Send + 'static>(mut stream: R) -> SharedData {
        let out = Arc::new(Mutex::new(Vec::new()));
        let vec = out.clone();
        std::thread::Builder::new()
            .name("child_stream_to_vec".into())
            .spawn(move || loop {
                let mut buf = [0];
                match stream.read(&mut buf) {
                    Err(err) => {
                        dbg!(err);
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
}
// output:1 ends here

// [[file:../vasp-tools.note::*stdin][stdin:1]]
pub struct StdinWriter {
    stdin: ChildStdin,
}

impl StdinWriter {
    pub fn new(stdin: ChildStdin) -> Self {
        Self { stdin }
    }

    /// Write `input` into self's stdin
    pub async fn write(&mut self, input: &str) -> Result<()> {
        use io::AsyncWriteExt;

        debug!("will write {:} bytes into stdin", input.len());
        self.stdin.write_all(dbg!(input).as_bytes()).await?;
        self.stdin.flush().await?;
        debug!("wrote stdin done");

        Ok(())
    }
}

mod stdin {
    use gut::prelude::*;
    use std::process::ChildStdin;

    pub struct StdinWriter {
        stdin: ChildStdin,
    }

    impl StdinWriter {
        pub fn new(stdin: ChildStdin) -> Self {
            Self { stdin }
        }

        /// Write `input` into self's stdin
        pub fn write(&mut self, input: &str) -> Result<()> {
            use std::io::prelude::*;

            debug!("will write {:} bytes into stdin", input.len());
            self.stdin.write_all(dbg!(input).as_bytes())?;
            self.stdin.flush()?;

            Ok(())
        }
    }
}
// stdin:1 ends here
