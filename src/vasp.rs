// [[file:../vasp-server.note::*INCAR file][INCAR file:1]]
use gut::prelude::*;
use std::path::Path;

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
    write_to_file(path, "LABORT = .TRUE.\n")?;

    Ok(())
}

pub(crate) fn stop_vasp_server() -> Result<()> {
    write_stopcar()?;

    Ok(())
}
// STOPCAR:1 ends here
