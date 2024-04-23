use std::path::PathBuf;

use clap::{arg, ArgAction};

// invoked as cargo plugin
fn from_cargo() -> bool {
    // oncelock?
    std::env::args()
        .next()
        .map_or(false, |name| name == ("cargo"))
}

fn cli() -> clap::Command {
    if from_cargo() {
        return clap::Command::new("cargo")
            .bin_name("cargo")
            .subcommand_required(true)
            .subcommand(cmd());
    } else {
        return cmd();
    }
}
pub fn args_for_cargo() -> Vec<String> {
    let skip = if from_cargo() { 2 } else { 1 };
    std::env::args()
        .skip(skip)
        .fold(Vec::new(), |mut acc, arg| {
            if !build_dir()
                .ok()
                .map_or(false, |dir| PathBuf::from(arg.clone()) == dir)
            {
                acc.push(arg)
            }
            acc
        })
}

pub fn parse() -> clap::ArgMatches {
    cli().get_matches()
}

fn cmd() -> clap::Command {
    clap::command!("ninja")
        .about("Generate `build.ninja` for `cargo build`.")
        .arg(
            arg!(<BUILD_DIR> "Where to put the generated `build.ninja`")
                .value_parser(clap::value_parser!(std::path::PathBuf)),
        )
        .arg(arg!(-Z <FLAG> "Unstable (nightly-only) flags to Cargo, see 'cargo -Z help' for details)"))
        .next_help_heading("Package Selection")
        .arg(arg!(-p --package <SPEC>  "Package to build (see `cargo help pkgid`)").num_args(0..=1)
        .action(ArgAction::Append))
        .arg(arg!(--workspace         "Build all packages in the workspace"))
        .arg(arg!(--exclude <SPEC>    "Exclude packages from the build"))
        .arg(arg!(--all               "Alias for --workspace (deprecated)"))
        // Target Selection:
        .next_help_heading("Target Selection")
        .arg(arg!(--lib               "Build only this package's library"))
        .arg(arg!(--bins              "Build all binaries"))
        .arg(arg!(--bin  <NAME>       "Build only the specified binary").num_args(0..=1))
        .arg(arg!(--examples          "Build all examples"))
        .arg(arg!(--example  <NAME>   "Build only the specified example").num_args(0..=1))
        .arg(arg!(--tests             "Build all test targets"))
        .arg(arg!(--test  <NAME>      "Build only the specified test target").num_args(0..=1))
        .arg(arg!(--benches           "Build all bench targets"))
        .arg(arg!(--bench  <NAME>     "Build only the specified bench target").num_args(0..=1))
        .arg(arg!(--"all-targets"     "Build all targets"))
        .next_help_heading("Feature Selection")
        .arg(arg!(-F --features <FEATURES>  "Space or comma separated list of features to activate"))
        .arg(arg!(--"all-features"     "Activate all available features"))
        .arg(arg!(--"no-default-features"     "Do not activate the `default` feature"))
        .next_help_heading("Compilation Options")
        .arg(arg!(-r --release                 "Build artifacts in release mode, with optimizations"))
        .arg(arg!(--profile <"PROFILE-NAME">  "Build artifacts with the specified profile"))
        .arg(arg!(--target <TRIPLE>       "Build for the target triple").num_args(0..=1))
        .arg(arg!(--timings <FMTS>        "Timing output formats (unstable) (comma separated): html, json").num_args(0..=1).require_equals(true))
        .next_help_heading("Manifest Options")
        .arg(arg!(--"manifest-path" <PATH>  "Path to Cargo.toml"))
        .arg(arg!(--frozen                "Require Cargo.lock and cache are up to date"))
        .arg(arg!(--locked                "Require Cargo.lock is up to date"))
        .arg(arg!(--offline               "Run without accessing the network"))
        .after_help("Run `cargo help build` for more detailed information.")
}

pub fn build_dir() -> anyhow::Result<PathBuf> {
    parse()
        .get_one::<std::path::PathBuf>("BUILD_DIR")
        .map(|p| p.clone())
        .ok_or(anyhow::format_err!("BUILD_DIR None"))
}
