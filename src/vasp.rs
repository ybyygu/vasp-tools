// [[file:../vasp-server.note::*INCAR][INCAR:1]]
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
// INCAR:1 ends here
