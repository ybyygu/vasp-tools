// [[file:../vasp-tools.note::f89cd5b2][f89cd5b2]]
use super::*;

use duct::cmd;
use gut::prelude::*;
// f89cd5b2 ends here

// [[file:../vasp-tools.note::5e88e23c][5e88e23c]]
pub struct AsciiPlot {
    xlabel: String,
    ylabel: String,
    title: String,
}

impl AsciiPlot {
    pub fn new() -> Self {
        Self {
            xlabel: "default xlabel".into(),
            ylabel: "default ylabel".into(),
            title: "default title".into(),
        }
    }

    pub fn set_xlabel(&mut self, label: &str) {
        self.xlabel = label.into();
    }

    pub fn set_ylabel(&mut self, label: &str) {
        self.ylabel = label.into();
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.into();
    }

    pub fn plot(&self, x: &[f64], y: &[f64]) -> Result<String> {
        // data file for gnuplot input
        let data_file = "plot.dat";

        let mut plot_script = String::new();
        writeln!(&mut plot_script, "set terminal dumb")?;
        writeln!(&mut plot_script, "set title \"{}\"", self.title)?;
        writeln!(&mut plot_script, "set xlabel \"{}\"", self.xlabel)?;
        writeln!(&mut plot_script, "set ylabel \"{}\"", self.ylabel)?;
        writeln!(&mut plot_script, "set format y \"%-0.2f\"")?;
        writeln!(&mut plot_script, "set tics scale 0")?;
        writeln!(&mut plot_script, "unset key")?;
        writeln!(&mut plot_script, "plot \"{}\" using 1:2 with dots", data_file)?;

        // create data file in temp dir
        // Create a directory inside of `std::env::temp_dir()`
        let dir = tempfile::tempdir()?;
        let file = dir.path().join(data_file);
        let data: String = x.iter().zip(y).map(|(_x, _y)| format!("{}\t{}\n", _x, _y)).collect();
        gut::fs::write_to_file(file, &data)?;

        let output = duct::cmd!("gnuplot").dir(dir.path()).stdin_bytes(plot_script.as_str()).read()?;
        Ok(output)
    }
}
// 5e88e23c ends here

// [[file:../vasp-tools.note::ac52b11c][ac52b11c]]
#[test]
#[ignore]
fn test_gnuplot_ascii_plot() {
    let mut ascii_plot = AsciiPlot::new();
    ascii_plot.set_title("Geometry optimization");
    ascii_plot.set_xlabel("energy (eV)");
    ascii_plot.set_ylabel("opt. step");

    let y = vec![
        -369.604028,
        -369.700139,
        -369.708766,
        -369.739834,
        -369.804727,
        -369.809632,
        -369.828092,
        -369.856902,
        -369.943526,
        -370.076070,
        -369.421450,
    ];
    let x: Vec<_> = (0..y.len()).map(|x| x as f64).collect();

    let s = ascii_plot.plot(&x, &y).unwrap();
    println!("{}", s);
}
// ac52b11c ends here
