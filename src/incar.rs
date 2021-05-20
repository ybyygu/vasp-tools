// [[file:../vasp-tools.note::*docs][docs:1]]
//! Handle VASP INCAR file
// docs:1 ends here

// [[file:../vasp-tools.note::*imports][imports:1]]
use gut::prelude::*;

use std::path::{Path, PathBuf};
// imports:1 ends here

// [[file:../vasp-tools.note::*update params][update params:1]]
/// Return updated parameters in INCAR file with new `params`.
pub fn update_with_mandatory_params(path: &Path, params: &[&str]) -> Result<String> {
    // INCAR file may contains invalid UTF-8 characters, so we handle it using
    // byte string
    use bstr::{ByteSlice, B};

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
    lines.push(B("# Mandatory parameters for VASP server:"));
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
// update params:1 ends here
