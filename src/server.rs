// [[file:../vasp-server.note::*imports][imports:1]]
use gchemol::Molecule;
use gosh::gchemol;
use gosh::model::ModelProperties;
use gut::prelude::*;

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

    /// unique temporary working directory
    temp_dir: Option<TempDir>,

    task: Option<crate::task::Task>,

    /// Record the number of potential evalulations.
    ncalls: usize,
}
// base:1 ends here

// [[file:../vasp-server.note::*env][env:1]]
impl VaspServer {
    fn from_dotenv(dir: &Path) -> Result<Self> {
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
            temp_dir: None,
            task: None,
            ncalls: 0,
        };
        bbm.prepare_compute_env()?;
        Ok(bbm)
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
// env:1 ends here

// [[file:../vasp-server.note::*prepare][prepare:1]]
use tempfile::{tempdir, tempdir_in, TempDir};

impl VaspServer {
    /// Return a temporary directory under `BBM_SCR_ROOT` for safe calculation.
    fn new_scratch_directory(&self) -> Result<TempDir> {
        let tdir = if let Some(ref scr_root) = self.scr_dir {
            trace!("set scratch root directory as: {:?}", scr_root);
            tempdir_in(scr_root)?
        } else {
            let tdir = tempdir()?;
            debug!("scratch root directory is not set, use the system default.");
            tdir
        };
        info!("BBM scratching directory: {:?}", tdir);
        Ok(tdir)
    }

    /// Prepare computation env
    fn prepare_compute_env(&mut self) -> Result<()> {
        trace!("calling script file: {:?}", self.run_file);

        // re-use the same scratch directory for multi-step calculation, e.g.
        // optimization.
        let mut tdir_opt = self.temp_dir.take();
        let tdir = tdir_opt.get_or_insert_with(|| {
            self.new_scratch_directory()
                .with_context(|| format!("Failed to create scratch directory"))
                .unwrap()
        });
        let ptdir = tdir.path();

        trace!("scratch dir: {}", ptdir.display());

        let tpl_dir = self
            .tpl_file
            .parent()
            .ok_or(format_err!("bbm_tpl_file: invalid path: {:?}", self.tpl_file))?;

        trace!("BBM_TPL_DIR: {:?}", tpl_dir);
        let cdir = std::env::current_dir()?;
        trace!("BBM_JOB_DIR: {:?}", cdir);

        let cmdline = format!("{}", self.run_file.display());
        trace!("submit cmdline: {}", cmdline);
        // FIXME: export BBM_*_DIR vars to child commands
        self.task = crate::task::Task::new(&cmdline).into();

        // for re-using the scratch directory
        self.temp_dir = tdir_opt;

        Ok(())
    }
}
// prepare:1 ends here

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
        let task = self.task.as_mut().expect("vasp task");
        let mp = task.compute_mol(mol)?;
        self.ncalls += 1;

        Ok(mp)
    }
}
// pub/chemical model:1 ends here

// [[file:../vasp-server.note::*test][test:1]]
#[test]
fn test_bbm_vasp_server() -> Result<()> {
    gut::cli::setup_logger_for_test();
    let d = "./tests/files/live-vasp";
    let mut vasp = VaspServer::from_dir(d)?;
    dbg!(vasp);

    Ok(())
}
// test:1 ends here
