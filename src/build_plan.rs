//! A parser for Cargo's `--build-plan` output.
//!
//! The main type is [`BuildPlan`]. To parse Cargo's output into a `BuildPlan`, call
//! [`BuildPlan::from_cargo_output`].
//!
//! [`BuildPlan`]: struct.BuildPlan.html
//! [`BuildPlan::from_cargo_output`]: struct.BuildPlan.html#method.from_cargo_output

#![warn(missing_debug_implementations)]

use serde::de::{self, Error};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitStatus;

/// Kinds of libraries that can be created.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LibKind {
    Lib,
    Rlib,
    Dylib,
    ProcMacro,
    Other(String),
}

impl LibKind {
    pub fn from_str(string: &str) -> LibKind {
        match string {
            "lib" => LibKind::Lib,
            "rlib" => LibKind::Rlib,
            "dylib" => LibKind::Dylib,
            "proc-macro" => LibKind::ProcMacro,
            s => LibKind::Other(s.to_string()),
        }
    }

    /// Returns the argument suitable for `--crate-type` to pass to rustc.
    pub fn crate_type(&self) -> &str {
        match *self {
            LibKind::Lib => "lib",
            LibKind::Rlib => "rlib",
            LibKind::Dylib => "dylib",
            LibKind::ProcMacro => "proc-macro",
            LibKind::Other(ref s) => s,
        }
    }

    pub fn linkable(&self) -> bool {
        match *self {
            LibKind::Lib | LibKind::Rlib | LibKind::Dylib | LibKind::ProcMacro => true,
            LibKind::Other(..) => false,
        }
    }
}

/// Describes artifacts that can be produced using `cargo build`.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetKind {
    Lib(Vec<LibKind>),
    Bin,
    Test,
    Bench,
    ExampleLib(Vec<LibKind>),
    ExampleBin,
    CustomBuild,
}

impl<'de> de::Deserialize<'de> for TargetKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        use self::TargetKind::*;

        let raw = Vec::<&str>::deserialize(deserializer)?;
        Ok(match *raw {
            [] => return Err(D::Error::invalid_length(0, &"at least one target kind")),
            ["bin"] => Bin,
            ["example"] => ExampleBin, // FIXME ExampleLib is never created this way
            ["test"] => Test,
            ["custom-build"] => CustomBuild,
            ["bench"] => Bench,
            ref lib_kinds => Lib(lib_kinds.iter().cloned().map(LibKind::from_str).collect()),
        })
    }
}

/// A tool invocation.
#[derive(Debug, Deserialize)]
pub struct Invocation {
    pub package_name: String,
    pub target_kind: TargetKind,
    pub compile_mode: String,
    /// List of invocations this invocation depends on.
    ///
    /// The vector contains indices into the [`BuildPlan::invocations`] list.
    ///
    /// [`BuildPlan::invocations`]: struct.BuildPlan.html#structfield.invocations
    pub deps: Vec<usize>,
    /// List of output artifacts (binaries/libraries) created by this invocation.
    pub outputs: Vec<PathBuf>,
    /// Hardlinks of output files that should be placed.
    pub links: BTreeMap<PathBuf, PathBuf>,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl Invocation {
    pub fn exec(&self) {
        use std::io::{self, Write};
        use std::process::Command;
        // if !self.outputs.is_empty() && self.outputs.iter().all(|o| o.exists()) {
        //     println!("extists");
        //     return;
        // };
        // .dwp
        // DWARF Package (dwp)
        for output in self.outputs.clone() {
            if let Some(dir) = output.as_path().parent() {
                fs::create_dir_all(dir).expect("failed to create dir");
            }
        }

        let output = Command::new(self.program.clone())
            .current_dir(self.cwd.clone().unwrap())
            .args(self.args.clone())
            .envs(self.env.clone())
            .output()
            .expect("failed to execute process");

        // println!("status: {}", output.status);
        if output.status.success() {
            for (link, original) in self.links.clone() {
                if let Some(dir) = original.as_path().parent() {
                    fs::create_dir_all(dir).expect("failed to create dir");
                }
                // println!("{link:?} {original:?}");
                if link.exists() {
                    fs::remove_file(link.clone()).expect("failed to remove old link")
                }
                if original.exists() {
                    fs::hard_link(original, link).expect("failed to create link");
                    // Hard link a.txt to b.txt
                }
            }
        }
        // io::stdout().write_all(&output.stdout).unwrap();
        io::stderr().write_all(&output.stderr).unwrap();
    }
}

/// A build plan output by `cargo build --build-plan`.
#[derive(Debug, Deserialize)]
pub struct BuildPlan {
    /// Program invocations needed to build the target (along with dependency information).
    pub invocations: Vec<Invocation>,
    /// List of Cargo manifests involved in the build.
    pub inputs: Vec<PathBuf>,
}

impl BuildPlan {
    /// Parses a `BuildPlan` from Cargo's JSON output.
    ///
    /// Build plan output can be obtained by running `cargo build --build-plan`. Generating build
    /// plans for individual targets (tests, examples, etc.) also works.
    pub fn from_cargo_output<S: AsRef<[u8]>>(output: S) -> serde_json::Result<Self> {
        serde_json::from_slice(output.as_ref())
    }
}
