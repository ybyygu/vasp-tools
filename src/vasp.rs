// [[file:../vasp-tools.note::*docs][docs:1]]
//! Utilities for handling VASP input/output
// docs:1 ends here

// [[file:../vasp-tools.note::89f26dfd][89f26dfd]]
use super::*;
// 89f26dfd ends here

// [[file:../vasp-tools.note::*mods][mods:1]]
mod freq;
// mods:1 ends here

// [[file:../vasp-tools.note::*pub][pub:1]]
pub use freq::VaspOutcar;
// pub:1 ends here

// [[file:../vasp-tools.note::*update params][update params:1]]
/// Handle VASP INCAR file
pub mod incar {
    use super::*;

    /// Return updated parameters in INCAR file with new `params`.
    pub fn update_with_mandatory_params(path: &Path, params: &[&str]) -> Result<String> {
        // INCAR file may contains invalid UTF-8 characters, so we handle it using
        // byte string
        use bstr::{ByteSlice, B};

        // remove mandatory tags defined by user, so we can add the required
        // parameters later
        let bytes = std::fs::read(path).with_context(|| format!("read {:?} file failure", path))?;
        let mut lines: Vec<&[u8]> = bytes
            .lines()
            .filter(|line| {
                let s = line.trim_start();
                if !s.starts_with_str("#") && s.contains_str("=") {
                    let parts: Vec<_> = s.splitn_str(2, "=").collect();
                    if parts.len() == 2 {
                        let tag = parts[0].trim().to_uppercase();
                        for param in params.iter() {
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
        // lines.push(B("# Mandatory parameters for VASP server:"));
        for param in params.iter() {
            lines.push(B(param));
        }
        let txt = bstr::join("\n", &lines).to_str_lossy().into();

        Ok(txt)
    }

    #[test]
    #[ignore]
    fn test_update_incar() -> Result<()> {
        let mandatory_params = vec![
            "POTIM = 0",
            "NELM = 200",
            "NSW = 0",
            "IBRION = -1",
            "ISYM = 0",
            "INTERACTIVE = .TRUE.",
        ];

        let s = update_with_mandatory_params("./tests/files/INCAR".as_ref(), &mandatory_params)?;
        gut::fs::write_to_file("/tmp/INCAR_new", &s)?;

        Ok(())
    }
}
// update params:1 ends here

// [[file:../vasp-tools.note::57803ca9][57803ca9]]
#[derive(Debug, Clone)]
pub enum VaspTask {
    Interactive,
    SinglePoint,
    Frequency,
}

/// Update INCAR file in current directory for BBM calculation
pub fn update_incar_for_bbm(task: &VaspTask) -> Result<()> {
    debug!("Update INCAR for VASP calculation: task = {:?}", task);

    let mandatory_params = task.mandatory_params();
    let updated_incar = crate::vasp::incar::update_with_mandatory_params("INCAR".as_ref(), &mandatory_params)?;
    gut::fs::write_to_file("INCAR", &updated_incar)?;

    Ok(())
}

impl VaspTask {
    fn mandatory_params(&self) -> Vec<&str> {
        let interactive_params = vec![
            "EDIFFG = -1E-5", // a small enough value is required to prevent early exit of VASP
            "NSW = 99999",    // a large enough value is required to prevent early exit of VASP
            "IBRION = -1",    // for static energy/force calculation
            "NWRITE = 1",     // setting NWRITE=0 could missing energy/forces in OUTCAR or stdout
            "NELMIN=10",      // insure accuracy when input a close structure to the previous step
            "INTERACTIVE = .TRUE.",
            "LCHARG = .FALSE.", // avoid creating large files
            "LWAVE  = .FALSE.",
            "POTIM = 0",
            "ISYM = 0",
        ];

        let single_point_params = vec![
            "EDIFFG = -1E-5", // a small enough value is required to prevent early exit of VASP
            "NSW = 0",        // one time single point calculation for energy and forces
            "IBRION = -1",    // for static energy/force calculation
            "NWRITE = 1",     // setting NWRITE=0 could missing energy/forces in OUTCAR or stdout
            "INTERACTIVE = .FALSE.",
            "POTIM = 0",
            "ISYM = 0",
        ];

        // remove NPAR and NCORE?
        let frequency_params = vec![
            "EDIFFG = -1E-5", // a small enough value is required to prevent early exit of VASP
            "NSW = 1",        // one time single point calculation for energy and forces
            "NFREE = 2",
            "POTIM = 0.015",
            "IBRION = 5",
            "INTERACTIVE = .FALSE.",
            "LCHARG = .FALSE.", // avoid creating large files
            "LWAVE  = .FALSE.",
        ];

        match self {
            Self::Interactive => interactive_params,
            Self::SinglePoint => single_point_params,
            Self::Frequency => frequency_params,
        }
    }
}
// 57803ca9 ends here

// [[file:../vasp-tools.note::*poscar][poscar:1]]
/// Handle VASP POSCAR file
pub mod poscar {
    use super::*;

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
}
// poscar:1 ends here

// [[file:../vasp-tools.note::*stopcar][stopcar:1]]
/// The STOPCAR file for stopping interactive calculation.
pub mod stopcar {
    use super::*;

    pub fn write(wrk_dir: &Path) -> Result<()> {
        debug!("Writing STOPCAR ...");
        gut::fs::write_to_file(wrk_dir.join("STOPCAR"), "LABORT = .TRUE.\n").context("write STOPCAR")?;

        Ok(())
    }
}
// stopcar:1 ends here

// [[file:../vasp-tools.note::*stdin][stdin:1]]
/// Handle text from stdin
pub mod stdin {
    use super::*;

    fn get_scaled_positions_from_poscar_str(s: &str) -> Result<String> {
        use gosh::gchemol::prelude::*;
        use gosh::gchemol::Molecule;

        let frac_coords: String = Molecule::from_str(s, "vasp/input")?
            .get_scaled_positions()
            .ok_or(format_err!("non-periodic structure?"))?
            .map(|[x, y, z]| format!("{:19.16} {:19.16} {:19.16}\n", x, y, z))
            .collect();

        Ok(frac_coords)
    }

    /// Read scaled positions from current process's standard input
    pub fn get_scaled_positions_from_stdin() -> Result<String> {
        let txt = read_txt_from_stdin()?;
        get_scaled_positions_from_poscar_str(&txt)
    }

    /// Read text from current process's standard input
    pub fn read_txt_from_stdin() -> Result<String> {
        use std::io::{self, Read};

        let mut buffer = String::new();
        let mut stdin = io::stdin(); // We get `Stdin` here.
        stdin.read_to_string(&mut buffer)?;
        Ok(buffer)
    }
}
// stdin:1 ends here

// [[file:../vasp-tools.note::*stdout][stdout:1]]
/// Parse energy and forces from VASP stdout when run in interactive mode
pub mod stdout {
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
        let mut tag_forces = tag("FORCES:");
        let mut read_forces = many1(read_xyz);

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

// [[file:../vasp-tools.note::0cf24c08][0cf24c08]]
pub mod outcar {
    use super::*;

    use gchemol::prelude::*;
    use gchemol::Molecule;
    use gosh::gchemol;
    use text_parser::TextReader;

    #[derive(Debug, Default, Clone)]
    struct OptIter {
        i: usize,
        energy: Option<f64>,
        // number of SCF for this opt step
        nscf: Option<usize>,
        volume: Option<f64>,
        mag: Option<f64>,
        fmax: Option<f64>,
    }

    /// Parse OUTCAR file
    pub fn summarize_outcar(f: &Path, plot: bool) -> Result<()> {
        use std::io::BufRead;

        let r = TextReader::from_path(f)?;
        let mut parts = r.partitions_preceded(|line| line.contains("FREE ENERGIE OF THE ION-ELECTRON SYSTEM"));

        // read selective dynamics flags from POSCAR of CONTCAR
        let fposcar = f.with_file_name("POSCAR");
        let fcontcar = f.with_file_name("CONTCAR");
        let mol: Molecule = if fposcar.exists() {
            Molecule::from_file(&fposcar)?
        } else if fcontcar.exists() {
            Molecule::from_file(&fcontcar)?
        } else {
            bail!("no POSCAR of CONTCAR");
        };

        let mut old_partition = parts.next().ok_or(format_err!("OUTCAR has no partition"))?;
        let mut collected_parts = vec![];
        for (i, p) in parts.skip(1).enumerate() {
            // the first part has no energy. we have to parse forces from the previous partition
            //
            // FREE ENERGIE OF THE ION-ELECTRON SYSTEM (eV)
            // ---------------------------------------------------
            // free  energy   TOTEN  =      -402.83834064 eV
            //
            // energy  without entropy=     -402.84358808  energy(sigma->0) =     -402.84008979
            let mut part = OptIter::default();
            part.i = i;
            part.fmax = read_forces_and_fmax(&old_partition, &mol);
            let mut nscf = 0;
            for line in p.lines() {
                if line.contains("free  energy   TOTEN  =") {
                    let attrs: Vec<_> = line.split_whitespace().collect();
                    if attrs.len() != 6 {
                        bail!("unexpected line: {:?}", attrs);
                    }
                    part.energy = attrs[4].parse().ok();
                } else if line.contains("-- Iteration") {
                    nscf += 1;
                } else if line.contains("volume of cell :") {
                    let attrs: Vec<_> = line.split_whitespace().collect();
                    assert_eq!(attrs.len(), 5);
                    part.volume = attrs[4].parse().ok();
                } else if line.starts_with(" number of electron") {
                    //  number of electron     699.9999451 magnetization     114.0418239
                    let attrs: Vec<_> = line.split_whitespace().collect();
                    assert!(attrs.len() >= 5, "{:?}", attrs);
                    if attrs.len() > 5 {
                        part.mag = attrs[5].parse().ok();
                    }
                }
            }
            old_partition = p;
            part.nscf = nscf.into();
            // show_iter(&part);
            collected_parts.push(part);
        }
        if plot {
            use crate::plot::AsciiPlot;
            let mut ascii_plot = AsciiPlot::new();

            ascii_plot.set_title("Geometry optimization");
            ascii_plot.set_xlabel("opt. step");
            ascii_plot.set_ylabel("energy (eV)");
            let x = collected_parts.iter().map(|o| o.i as f64).collect_vec();
            let y = collected_parts.iter().map(|o| o.energy.unwrap() as f64).collect_vec();
            let s = ascii_plot.plot(&x, &y)?;
            println!("{}", s);
        } else {
            for part in collected_parts {
                show_iter(&part);
            }
        }
        Ok(())
    }

    fn read_forces_and_fmax(s: &str, mol: &Molecule) -> Option<f64> {
        use vecfx::*;

        let token = "TOTAL-FORCE (eV/Angst)";
        let natoms = mol.natoms();
        let mut r = TextReader::from_str(s);
        let _ = r.seek_line(|line| line.contains(token));
        let mut lines = r.lines().take(natoms + 2);
        let first_line = lines.next()?;
        if first_line.contains(token) {
            //      -0.04844      0.25073      4.19570         0.005351      0.001537     -0.846521
            let forces: Vec<f64> = lines
                .skip(1)
                .flat_map(|line| {
                    let f3: Vec<_> = line.split_whitespace().skip(3).map(|x| x.parse().unwrap()).collect();
                    f3
                })
                .collect();
            let mask = mol.freezing_coords_mask();
            let forces_masked = mask.apply(&forces);
            let fmax = forces_masked.as_3d().iter().map(|x| x.vec2norm()).float_max();
            fmax.into()
        } else {
            None
        }
    }

    fn show_iter(p: &OptIter) {
        let e = p.energy.map(|e| format!("{:.6}", e)).unwrap_or(format!("{:}", "--"));
        let fmax = p.fmax.map(|f| format!("{:.6}", f)).unwrap_or(format!("{:4}", "--"));
        let nscf = p.nscf.map(|n| format!("{:4}", n)).unwrap_or(format!("{:4}", "--"));
        let mag = p.mag.map(|m| format!("{:.2}", m)).unwrap_or(format!("{:4}", "--"));
        println!(
            "{:<6} Energy: {:12} fmax: {:12} SCF: {:} Mag: {:6}",
            p.i, e, fmax, nscf, mag
        );
    }

    #[test]
    #[ignore]
    fn test_outcar_parser() {
        summarize_outcar("tests/files/OUTCAR".as_ref(), false);
    }
}
// 0cf24c08 ends here
