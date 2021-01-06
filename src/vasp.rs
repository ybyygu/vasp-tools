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
        let bbm = VaspServer {
            run_file: dir.join(run_file),
            tpl_file: dir.join(tpl_file),
            scr_dir: envfile.get("BBM_SCR_DIR").map(|x| x.into()),
            temp_dir: None,
            task: None,
            ncalls: 0,
        };
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

    fn prepare_compute_env(&mut self) -> Result<()> {
        todo!()
    }
}
// env:1 ends here

// [[file:../vasp-server.note::*call][call:1]]
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

    /// Call external script
    fn safe_call(&mut self, input: &str) -> Result<String> {
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
        let cmd = gut::cli::duct::cmd!(&cmdline)
            .dir(ptdir)
            .env("BBM_TPL_DIR", tpl_dir)
            .env("BBM_JOB_DIR", cdir)
            .stdin_bytes(input);

        // for re-using the scratch directory
        self.temp_dir = tdir_opt;

        let stdout = cmd.read().context("BBM calling script failed.")?;

        self.ncalls += 1;

        Ok(stdout)
    }

    /// Test if task is ready for interactive
    fn is_interactive(&self) -> bool {
        self.task.is_some()
    }

    fn start_or_interact(&mut self) -> Result<()> {
        // For the first time, we should prepare an unique temp directory for
        // VASP calculation (INCAR, POTCAR, ...)
        if !self.is_interactive() {
            self.prepare_compute_env()?;
        }

        todo!()
    }
}
// call:1 ends here

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
        // 1. 启动VASP进程或与已开vasp进程交互
        self.start_or_interact()?;

        // 2. 将当前mol结构发送给VASP, 等待计算结果
        let task = self.task.as_mut().expect("vasp task");
        let mp = task.compute_mol(mol)?;

        Ok(mp)
    }
}
// pub/chemical model:1 ends here

// [[file:../vasp-server.note::*INCAR file][INCAR file:1]]
use gut::prelude::*;

#[derive(Debug, Clone)]
struct INCAR {
    // in tag = value pair
    params: Vec<(String, String)>,
}

impl INCAR {
    /// Read VASP INCAR from `path`
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        // INCAR中可能会含有中文字符, 或者无效的UTF-8字符
        use bstr::{ByteSlice, ByteVec};

        let bytes = std::fs::read(path)?;
        let lines: Vec<&[u8]> = bytes.lines().filter(|line| line.contains_str("=")).collect();

        let mut final_lines = String::new();
        // 首先剔除所有"#"号开头的注释
        for line in lines {
            if let Some(i) = line.find("#") {
                line[0..i].to_str_lossy_into(&mut final_lines);
            } else {
                line[..].to_str_lossy_into(&mut final_lines);
            }
            final_lines += "\n";
        }

        let mut params: Vec<(String, String)> = vec![];
        for line in final_lines.lines() {
            let s: Vec<_> = line.splitn(2, "=").collect();
            // 变成大写的TAG
            let tag = s[0].trim();
            // 同一行可以出现多个tag=value组合, 中间用"；"分隔
            let value = s[1].trim();
            if value.contains(";") {
                warn!("; found. that is not supported.")
            }
            params.push((tag.to_uppercase(), value.to_string()));
        }
        let incar = Self { params };

        Ok(incar)
    }

    /// Save as INCAR file
    pub fn save(&self) -> Result<()> {
        let n = self
            .params
            .iter()
            .map(|(tag, _)| tag.len())
            .max()
            .expect("INCAR: no lines");

        let lines: String = self
            .params
            .iter()
            .map(|(tag, value)| format!("{:n$} = {}\n", tag, value, n = n))
            .collect();

        gut::fs::write_to_file("INCAR", &lines)?;
        Ok(())
    }
}

#[test]
#[ignore]
fn test_incar() -> Result<()> {
    let incar = INCAR::from_file("./tests/files/INCAR")?;
    dbg!(incar);

    Ok(())
}
// INCAR file:1 ends here

// [[file:../vasp-server.note::*update INCAR][update INCAR:1]]
fn update_vasp_incar_file(path: &Path) -> Result<()> {
    // INCAR file may contains invalid UTF-8 characters, so we handle it using
    // byte string
    use bstr::{ByteSlice, B};

    let mandatory_params = vec![
        "POTIM = 0",
        "NELM = 200",
        "NSW = 0",
        "IBRION = -1",
        "ISYM = 0",
        "INTERACTIVE = .TRUE.",
    ];

    // remove mandatory tags defined by user, so we can add the required
    // parameters later
    let bytes = std::fs::read(path)?;
    let mut lines: Vec<&[u8]> = bytes
        .lines()
        .filter(|line| {
            let s = line.trim_start();
            if !s.starts_with_str("#") && s.contains_str("=") {
                let parts: Vec<_> = s.splitn_str(2, "=").collect();
                if parts.len() == 2 {
                    let tag = parts[0].trim().to_uppercase();
                    for param in mandatory_params.iter() {
                        let param = param.as_bytes().as_bstr();
                        if param.starts_with(&tag) {
                            return false;
                        }
                    }
                }
            }
            true
        })
        .collect();

    // append mandatory parameters
    lines.push(B("# Mandatory parameters for VASP server:"));
    for param in mandatory_params.iter() {
        lines.push(B(param));
    }
    let txt = bstr::join("\n", &lines);
    println!("{}", txt.to_str_lossy());

    std::fs::write("/tmp/INCAR_new", txt)?;

    Ok(())
}

#[test]
#[ignore]
fn test_update_incar() -> Result<()> {
    update_vasp_incar_file("./tests/files/INCAR".as_ref())?;

    Ok(())
}
// update INCAR:1 ends here

// [[file:../vasp-server.note::*STOPCAR][STOPCAR:1]]
fn write_stopcar() -> Result<()> {
    gut::fs::write_to_file("STOPCAR", "LABORT = .TRUE.\n").context("write STOPCAR")?;

    Ok(())
}

pub(crate) fn stop_vasp_server() -> Result<()> {
    write_stopcar()?;

    Ok(())
}
// STOPCAR:1 ends here

// [[file:../vasp-server.note::*stdout][stdout:1]]
pub(crate) mod stdout {
    use super::*;
    use std::io::prelude::*;
    use text_parser::parsers::*;

    fn parse_vasp_energy(s: &str) -> Option<f64> {
        if s.len() < 42 {
            None
        } else {
            s[26..26 + 16].trim().parse().ok()
        }
    }

    #[test]
    fn test_parse_vasp_energy() {
        let s = "   1 F= -.84780990E+02 E0= -.84775142E+02  d E =-.847810E+02  mag=     3.2666";
        let (_, e) = read_energy(s).unwrap();
        assert_eq!(e, -0.84775142E+02);
    }

    // FORCES:
    //      0.2084558     0.2221942    -0.1762308
    //     -0.1742340     0.2172782     0.2304866
    //      0.2244132    -0.1794341     0.2106465
    //     -0.2907316    -0.2746548    -0.2782190
    //     -0.2941880    -0.0306001    -0.0141722
    fn read_forces(s: &str) -> IResult<&str, Vec<[f64; 3]>> {
        let tag_forces = tag("FORCES:");
        let read_forces = many1(read_xyz);

        do_parse!(
            s,
            tag_forces >> eol   >>     // FORCES:
            forces: read_forces >>     // forces in each line
            (forces)
        )
    }

    //      0.2084558     0.2221942    -0.1762308
    fn read_xyz(s: &str) -> IResult<&str, [f64; 3]> {
        do_parse!(
            s,
            space1 >> xyz: xyz_array >> read_line >> // ignore the remaining characters
            (xyz)
        )
    }

    //    1 F= -.85097948E+02 E0= -.85096866E+02  d E =-.850979E+02  mag=     2.9646
    //    2 F= -.85086257E+02 E0= -.85082618E+02  d E =-.850863E+02  mag=     2.9772
    // POSITIONS: reading from stdin
    fn read_energy(s: &str) -> IResult<&str, f64> {
        let tag_nf = tag("F=");
        let tag_e0 = tag("E0=");
        do_parse!(
            s,
            space0 >> digit1 >> space1 >> tag_nf >> space0 >> double >>  // 1 F= ...
            space0 >> tag_e0 >> space0 >> energy: double >> read_line >> // E0= ...
            (energy)
        )
    }

    fn read_energy_and_forces(s: &str) -> IResult<&str, (f64, Vec<[f64; 3]>)> {
        let jump = take_until("FORCES:\n");
        do_parse!(
            s,
            jump >>                 // skip leading text until found "FORCES"
            forces: read_forces >>  // read forces
            energy: read_energy >>  // read forces
            ((energy, forces))
        )
    }

    pub fn parse_energy_and_forces(s: &str) -> Result<(f64, Vec<[f64; 3]>)> {
        let (_, values) = read_energy_and_forces(s).unwrap();
        Ok(values)
    }

    #[test]
    fn test_parse_vasp_interactive() -> Result<()> {
        let s = "./tests/files/interactive.txt";
        let s = gut::fs::read_file(s)?;

        let (e, f) = parse_energy_and_forces(&s)?;
        assert_eq!(f.len(), 25);

        Ok(())
    }
}
// stdout:1 ends here
