// [[file:../vasp-server.note::*imports][imports:1]]
use gut::prelude::*;

use std::path::{Path, PathBuf};
// imports:1 ends here

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

// [[file:../vasp-server.note::*poscar][poscar:1]]
// read scaled positions from POSCAR
fn get_scaled_positions_from_poscar(path: &Path) -> Result<String> {
    let s = gut::fs::read_file(path)?;

    let lines: Vec<_> = s
        .lines()
        .skip_while(|line| !line.to_uppercase().starts_with("DIRECT"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .collect();
    let mut positions = lines.join("\n");
    // final line separator
    positions += "\n";
    Ok(positions)
}

#[test]
fn test_poscar_positions() -> Result<()> {
    let poscar = "./tests/files/live-vasp/POSCAR";

    let s = get_scaled_positions_from_poscar(poscar.as_ref())?;
    assert_eq!(s.lines().count(), 25);

    Ok(())
}

// pub fn get_scaled_positions() -> Result<String> {
//     let poscar: &Path = "POSCAR".as_ref();
//     let s = if poscar.exists() {
//         String::new()
//     } else {
//         get_scaled_positions_from_poscar(poscar)?
//     };

//     Ok(s)
// }
// poscar:1 ends here

// [[file:../vasp-server.note::*stopcar][stopcar:1]]
pub(crate) fn write_stopcar() -> Result<()> {
    gut::fs::write_to_file("STOPCAR", "LABORT = .TRUE.\n").context("write STOPCAR")?;

    Ok(())
}
// stopcar:1 ends here

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

    /// Parse energy and forces from stdout of VASP interactive calculation
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

// [[file:../vasp-server.note::*process][process:1]]
mod adhoc {
    use super::*;
    use duct::*;

    fn get_process_ctime(pid: usize) -> Result<i64> {
        let s = cmd!("ps", "-o", "lstart=", pid.to_string())
            .env("LC_TIME", "C") // ensure plain time format
            .read()?;

        // convert unix timestamp
        // date -d "Wed Jan 20 14:44:41 CST 2021" +%s --utc
        let d = cmd!("date", "-d", &s, "+%s", "--utc").read()?.parse()?;
        // let d = chrono::DateTime::parse_from_str(&s, "%c")?;
        // dbg!(d);

        Ok(d)
    }

    fn get_vasp_main_process_all() -> Result<Vec<usize>> {
        let p = cmd!("pgrep", "pmi_proxy").read()?;
        let pids = p.lines().map(|l| l.parse().unwrap()).collect();

        Ok(pids)
    }

    pub fn pause_process_group(ppid: usize) -> Result<()> {
        debug!("try to suspend process group {}", ppid);
        let _ = cmd!("pkill", "-SIGSTOP", "-P", ppid.to_string()).read()?;

        Ok(())
    }

    pub fn resume_process_group(ppid: usize) -> Result<()> {
        debug!("try to resume process group {}", ppid);
        let _ = cmd!("pkill", "-SIGCONT", "-P", ppid.to_string()).read()?;

        Ok(())
    }

    /// 找到匹配指定进程的VASP主进程
    pub fn find_vasp_main_process(ppid: usize) -> Result<usize> {
        let p_all = get_vasp_main_process_all()?;
        info!("found {} vasp main processes", p_all.len());

        // it is slow
        let ctime_all: Vec<_> = p_all.iter().map(|&p| get_process_ctime(p).ok().unwrap()).collect();
        let ctime1 = get_process_ctime(ppid)?;

        let tmp_abs_diffs = ctime_all
            .iter()
            .enumerate()
            .map(|(i, x)| ((x - ctime1).abs(), i))
            .sorted()
            .collect_vec();

        let i = tmp_abs_diffs[0].1;
        let main_vasp_pid = p_all[i];

        Ok(main_vasp_pid)
    }
}
// process:1 ends here

// [[file:../vasp-server.note::*pub/cli][pub/cli:1]]
mod cli {
    use super::*;
    use structopt::*;

    /// An adhoc helper program to control VASP mpi processes
    #[derive(Debug, StructOpt)]
    struct Cli {
        #[structopt(flatten)]
        verbose: gut::cli::Verbosity,

        /// The parent process id of VASP
        ppid: usize,

        #[structopt(long, short)]
        action: Option<String>,
    }

    pub fn enter_main() -> Result<()> {
        let args = Cli::from_args();
        args.verbose.setup_logger();

        let vasp_pid = adhoc::find_vasp_main_process(args.ppid)?;
        if let Some(action) = args.action {
            info!("{} process group {}", action, vasp_pid);
            match action.as_str() {
                "pause" => {
                    if let Err(err) = adhoc::pause_process_group(vasp_pid) {
                        dbg!(err);
                    }
                }
                "resume" => {
                    if let Err(err) = adhoc::resume_process_group(vasp_pid) {
                        dbg!(err);
                    }
                }
                _ => {
                    todo!();
                }
            }
        }

        Ok(())
    }
}
pub use cli::enter_main;
// pub/cli:1 ends here
