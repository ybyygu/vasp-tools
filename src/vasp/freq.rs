// [[file:../../vasp-tools.note::*imports][imports:1]]
use crate::common::*;

use text_parser::GrepReader;
use text_parser::TextReader;
// imports:1 ends here

// [[file:../../vasp-tools.note::*base][base:1]]
/// Represent a VASP produced OUTCAR file
#[derive(Debug, Default, Clone)]
pub struct VaspOutcar {
    natoms: Option<usize>,
    vibrational_mode: Option<Vec<[f64; 3]>>,
}
// base:1 ends here

// [[file:../../vasp-tools.note::*grep][grep:1]]
impl VaspOutcar {
    pub fn parse_last_imaginary_freq_mode_from(f: &Path) -> Result<Vec<[f64; 3]>> {
        let mut reader = GrepReader::try_from_path(f)?;
        let mut s = String::new();
        reader.read_lines(1, &mut s)?;

        if !parse::is_vasp_outcar_file(&s) {
            bail!("not a valid OUTCAR file!");
        }

        // number of dos      NEDOS =    301   number of ions     NIONS =     52
        // 21 f/i=   10.478975 THz    65.841344 2PiTHz  349.540982 cm-1    43.337574 meV
        let n = reader.mark(&[r"number of ions     NIONS =", r"^\s*\d+\s*f/i="])?;
        println!("set up {} markers", n);
        assert!(n >= 2, "at least one imaginary frequency required (n={})", n);
        s.clear();
        reader.goto_marker(0);
        reader.read_lines(1, &mut s)?;
        let natoms = parse::parse_number_of_atoms(dbg!(&s))?;

        s.clear();
        // take the last imaginary vibration mode
        for i in 0..(n - 1) {
            let _ = reader.goto_next_marker()?;
        }
        reader.read_lines(natoms + 2, &mut s)?;

        let vib = parse::parse_imaginary_vibrational_mode(&s, natoms)?;

        Ok(vib)
    }
}
// grep:1 ends here

// [[file:../../vasp-tools.note::*parse][parse:1]]
mod parse {
    use super::*;
    use text_parser::parsers::*;

    fn number_of_ions(s: &str) -> IResult<&str, usize> {
        let (s, _) = take_until("NIONS")(s)?;
        let (s, (_, n)) = tuple((tag("NIONS ="), ws(unsigned_digit)))(s)?;
        Ok((s, n))
    }

    // check the first line to make sure it is a OUTCAR file.
    pub fn is_vasp_outcar_file(s: &str) -> bool {
        // vasp.5.3.5 31Mar14 (build Aug 17 2020 07:42:27) complex
        s.starts_with(" vasp.")
    }

    // number of dos      NEDOS =    301   number of ions     NIONS =     52
    pub fn parse_number_of_atoms(s: &str) -> Result<usize> {
        let (_, n) = number_of_ions(s).map_err(|e| format_err!("parse NIONS failure: {:?}, {:}", e, s))?;
        Ok(n)
    }

    // 21 f/i=   10.478975 THz    65.841344 2PiTHz  349.540982 cm-1    43.337574 meV
    //        X         Y         Z           dx          dy          dz
    // 0.000000  0.000000  2.000078            0           0           0
    pub fn parse_imaginary_vibrational_mode(s: &str, natoms: usize) -> Result<Vec<[f64; 3]>> {
        let (_, vmode) =
            imaginary_vibrational_mode(s).map_err(|e| format_err!("parse f/i failure: {:?} {:}", e, s))?;
        Ok(vmode)
    }

    fn imaginary_vibrational_mode(s: &str) -> IResult<&str, Vec<[f64; 3]>> {
        // skip first two lines
        let (s, _) = read_line(s)?;
        let (s, _) = read_line(s)?;

        let vib_line = tuple((ws(xyz_array), ws(xyz_array), eol));
        let dxyz = map(vib_line, |(_, d, _)| d);
        many1(dxyz)(s)
    }

    #[test]
    fn test_parser() -> Result<()> {
        let s = "   number of dos      NEDOS =    301   number of ions     NIONS =     52\n";
        let n = parse_number_of_atoms(s)?;
        assert_eq!(n, 52, "{:?}", s);

        let s = "  21 f/i=   10.478975 THz    65.841344 2PiTHz  349.540982 cm-1    43.337574 meV
             X         Y         Z           dx          dy          dz
      0.000000  0.000000  2.000078            0           0           0  
     -0.022799  0.040757  8.573679            0           0           0  
      1.365074  0.804926  6.453952            0           0           0  
     -0.000000  1.564238  4.212194            0           0           0  
      2.709340  0.000000  2.000078            0           0           0  
";
        let x = parse_imaginary_vibrational_mode(s, n)?;
        assert_eq!(x.len(), 5, "{:?}", x);

        Ok(())
    }
}
// parse:1 ends here
