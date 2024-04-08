#[path = "../custom_build.rs"]
mod custom_build;

use std::path::{Path, PathBuf};

use custom_build::BuildOutput;

fn main() -> Result<(), anyhow::Error> {
    let args = std::env::args().skip(1).take(1).collect::<Vec<String>>();
    let path = args
        .get(0)
        .ok_or(anyhow::format_err!("no build output path specified"))?;
    let output = PathBuf::from(path);
    let dir = output
        .parent()
        .ok_or(anyhow::format_err!("failed to get output dir"))?;
    let build_output = BuildOutput::parse_file(&output, None, "", dir, dir, true, true, &None)?;
    let args = build_output
        .library_paths
        .iter()
        .fold(String::new(), |mut args, p| {
            if !args.is_empty() {
                args += " ";
            }
            args += format!("-L {}", p.display()).as_str();
            args
        });
    let args = build_output.library_links.iter().fold(args, |mut args, l| {
        if !args.is_empty() {
            args += " ";
        }
        args += format!("-l {}", l).as_str();
        args
    });

    let args = build_output
        .linker_args
        .iter()
        .fold(args, |mut args, (_lt, arg)| {
            // check the target, target should be pass from args
            // if lt.applies_to(&unit.target) {
            if !args.is_empty() {
                args += " ";
            }
            args += format!("-C link-arg={}", arg).as_str();
            args
        });

    let args = build_output.cfgs.iter().fold(args, |mut args, cfg| {
        if !args.is_empty() {
            args += " ";
        }
        args += format!("--cfg {}", cfg).as_str();
        args
    });

    let args = build_output
        .check_cfgs
        .iter()
        .enumerate()
        .fold(args, |mut args, (i, cfg)| {
            if !args.is_empty() {
                args += " ";
            }
            if i == 0 {
                args += "-Zunstable-options";
            }
            args += format!("--check-cfg {}", cfg).as_str();
            args
        });

    println!("build_output: {args:?}");
    Ok(())
}
