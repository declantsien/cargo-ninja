#[path = "../custom_build.rs"]
mod custom_build;

use std::path::{Path, PathBuf};

use custom_build::BuildOutput;

fn main() {
    // path: &Path,
    //     library_name: Option<String>,
    //     pkg_descr: &str,
    //     script_out_dir_when_generated: &Path,
    //     script_out_dir: &Path,
    //     extra_check_cfg: bool,
    //     nightly_features_allowed: bool,
    //     msrv: &Option<RustVersion>,
    let output = PathBuf::from("/home/declan/src/cargo-ninja/7962435182945837035");
    let dir = output.parent().unwrap();
    let build_output = BuildOutput::parse_file(&output, None, "", dir, dir, true, true, &None);
    println!("build_output: {build_output:?}");
}
