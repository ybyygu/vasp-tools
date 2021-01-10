// [[file:../vasp-server.note::*imports][imports:1]]
use gchemol::prelude::*;
use gchemol::Molecule;
use gosh::gchemol;
use gosh::model::ModelProperties;
use gut::prelude::*;
use tempfile::TempDir;

use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-server.note::*base][base:1]]
#[derive(Debug)]
pub struct VaspServer {
    /// Set the run script file for calculation.
    run_file: PathBuf,

    /// Set the template file for rendering molecule.
    tpl_file: PathBuf,

    /// Set the root directory for scratch files.
    scr_dir: Option<PathBuf>,

    /// Job starting directory
    job_dir: Option<PathBuf>,

    /// unique temporary working directory
    temp_dir: Option<TempDir>,

    task: Option<crate::task::Task>,

    /// Record the number of potential evalulations.
    ncalls: usize,
}
// base:1 ends here

// [[file:../vasp-server.note::*env][env:1]]
mod env {
    use super::*;
    use tempfile::{tempdir, tempdir_in};

    /// Return a temporary directory under `BBM_SCR_ROOT` for safe calculation.
    fn new_scratch_directory(scr_root: Option<&Path>) -> Result<TempDir> {
        // create leading directories
        if let Some(d) = &scr_root {
            if !d.exists() {
                std::fs::create_dir_all(d).context("create scratch root dir")?;
            }
        }
        scr_root.map_or_else(
            || tempdir().context("create temp scratch dir"),
            |d| tempdir_in(d).with_context(|| format!("create temp scratch dir under {:?}", d)),
        )
    }

    impl VaspServer {
        /// 生成临时目录, 生成执行脚本
        pub(super) fn prepare_compute_env(&mut self) -> Result<PathBuf> {
            use std::os::unix::fs::PermissionsExt;

            let tdir = new_scratch_directory(self.scr_dir.as_deref())?;
            info!("BBM scratching directory: {:?}", tdir);

            // copy run file to work/scratch directory, and make sure it is
            // executable
            let dest = tdir.path().join("run").canonicalize()?;
            std::fs::copy(&self.run_file, &dest)
                .with_context(|| format!("copy {:?} to {:?}", &self.run_file, &dest))?;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).context("chmod +x")?;

            self.temp_dir = tdir.into();

            Ok(dest)
        }

        pub(super) fn from_dotenv(dir: &Path) -> Result<Self> {
            // canonicalize the file paths
            let dir = dir
                .canonicalize()
                .with_context(|| format!("invalid template directory: {:?}", dir))?;

            // read environment variables from .env config if any
            let mut envfile = envfile::EnvFile::new(dir.join(".env")).unwrap();
            for (key, value) in &envfile.store {
                info!("found env var from {:?}: {}={}", &envfile.path, key, value);
            }

            let run_file = envfile.get("BBM_RUN_FILE").unwrap_or("submit.sh");
            let tpl_file = envfile.get("BBM_TPL_FILE").unwrap_or("input.hbs");
            let mut bbm = VaspServer {
                run_file: dir.join(run_file),
                tpl_file: dir.join(tpl_file),
                scr_dir: envfile.get("BBM_SCR_DIR").map(|x| x.into()),
                job_dir: std::env::current_dir()?.into(),
                temp_dir: None,
                task: None,
                ncalls: 0,
            };
            Ok(bbm)
        }

        /// Render input using template
        fn render_input(&self, mol: &Molecule) -> Result<String> {
            // render input text with external template file
            let txt = mol.render_with(&self.tpl_file)?;

            Ok(txt)
        }

        // Construct from environment variables
        // 2020-09-05: it is dangerous if we have multiple BBMs in the sample process
        // fn from_env() -> Self {
        //     match envy::prefixed("BBM_").from_env::<VaspServer>() {
        //         Ok(bbm) => bbm,
        //         Err(error) => panic!("{:?}", error),
        //     }
        // }
    }

    #[test]
    fn test_env() -> Result<()> {
        let d = new_scratch_directory(Some("/scratch/test".as_ref()))?;
        assert!(d.path().exists());
        let d = new_scratch_directory(None)?;
        assert!(d.path().exists());
        Ok(())
    }
}
// env:1 ends here

// [[file:../vasp-server.note::*cmd][cmd:1]]
mod cmd {
    use super::*;
    use std::process::{Command, Stdio};

    impl VaspServer {
        pub(super) fn is_server_started(&self) -> bool {
            self.task.is_some()
        }

        /// 调用执行脚本, 派生进程
        pub(super) fn submit_cmd(&mut self) -> Result<()> {
            let run_file = self.prepare_compute_env()?;
            trace!("call script file: {:?}", run_file);

            let tpl_dir = self
                .tpl_file
                .parent()
                .ok_or(format_err!("bbm_tpl_file: invalid path: {:?}", self.tpl_file))?;
            trace!("BBM_TPL_DIR: {:?}", tpl_dir);

            let cdir = std::env::current_dir()?;
            trace!("BBM_JOB_DIR: {:?}", cdir);

            let cmdline = format!("{}", run_file.display());
            trace!("submit cmdline: {}", cmdline);
            let tdir = run_file.parent().unwrap();
            let mut child = Command::new(&cmdline)
                .current_dir(tdir)
                .env("BBM_TPL_DIR", &tpl_dir)
                .env("BBM_JOB_DIR", &cdir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .with_context(|| format!("run script: {:?}", &cmdline))?;
            self.task = crate::task::Task::new(child).into();

            Ok(())
        }
    }
}
// cmd:1 ends here

// [[file:../vasp-server.note::*interact][interact:1]]
impl VaspServer {
    fn interact(&mut self, mol: &Molecule) -> Result<ModelProperties> {
        self.task.as_mut().unwrap().input_positions(mol)?;
        let mp = self.task.as_mut().expect("vasp task").compute_mol(mol)?;

        Ok(mp)
    }
}
// interact:1 ends here

// [[file:../vasp-server.note::*pub/methods][pub/methods:1]]
impl VaspServer {
    /// Construct VaspServer model under directory context.
    pub fn from_dir<P: AsRef<Path>>(dir: P) -> Result<Self> {
        Self::from_dotenv(dir.as_ref()).context("Initialize VaspServer failure.")
    }

    /// keep scratch files for user inspection of failure.
    pub fn keep_scratch_files(self) {
        if let Some(tdir) = self.temp_dir {
            let path = tdir.into_path();
            println!("Directory for scratch files: {}", path.display());
        } else {
            warn!("No temp dir found.");
        }
    }

    /// Return the number of potentail evaluations
    pub fn number_of_evaluations(&self) -> usize {
        self.ncalls
    }
}
// pub/methods:1 ends here

// [[file:../vasp-server.note::*pub/chemical model][pub/chemical model:1]]
use gosh::model::ChemicalModel;

impl ChemicalModel for VaspServer {
    fn compute(&mut self, mol: &Molecule) -> Result<ModelProperties> {
        if !self.is_server_started() {
            todo!();
        }

        let mp = self.interact(mol)?;
        self.ncalls += 1;

        Ok(mp)
    }
}
// pub/chemical model:1 ends here
