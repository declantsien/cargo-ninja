//! A parser for Cargo's `--build-plan` output.
//!
//! The main type is [`BuildPlan`]. To parse Cargo's output into a `BuildPlan`, call
//! [`BuildPlan::from_cargo_output`].
//!
//! [`BuildPlan`]: struct.BuildPlan.html
//! [`BuildPlan::from_cargo_output`]: struct.BuildPlan.html#method.from_cargo_output

#![warn(missing_debug_implementations)]

use camino::Utf8PathBuf;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// A tool invocation.
#[derive(Debug, Deserialize, Clone, Hash)]
pub struct Invocation {
    pub package_name: String,
    pub package_version: String,
    pub target_kind: Vec<String>,
    pub compile_mode: String,
    /// List of invocations this invocation depends on.
    ///
    /// The vector contains indices into the [`BuildPlan::invocations`] list.
    ///
    /// [`BuildPlan::invocations`]: struct.BuildPlan.html#structfield.invocations
    pub deps: Vec<usize>,
    /// List of output artifacts (binaries/libraries) created by this invocation.
    pub outputs: Vec<Utf8PathBuf>,
    /// Hardlinks of output files that should be placed.
    pub links: BTreeMap<Utf8PathBuf, Utf8PathBuf>,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<Utf8PathBuf>,
}

impl Invocation {
    pub fn links(&self) -> BTreeMap<Utf8PathBuf, Utf8PathBuf> {
        let links = self.links.clone();
        links
            .into_iter()
            .filter(|(_, target)| !target.extension().map_or(false, |e| e == "dwp"))
            .collect()
    }

    pub fn hash_string(&self) -> String {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        let hash = s.finish();
        hash.to_string()
    }

    pub fn outputs(&self) -> Vec<Utf8PathBuf> {
        let outputs = if self.outputs.is_empty() {
            vec![Utf8PathBuf::from(self.hash_string())]
        } else {
            self.outputs
                .clone()
                .into_iter()
                .filter(|output| !output.extension().map_or(false, |e| e == "dwp"))
                .collect()
        };
        outputs
    }
}

/// A build plan output by `cargo build --build-plan`.
#[derive(Debug, Deserialize)]
pub struct BuildPlan {
    /// Program invocations needed to build the target (along with dependency information).
    pub invocations: Vec<Invocation>,
    /// List of Cargo manifests involved in the build.
    pub inputs: Vec<Utf8PathBuf>,
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

pub fn with_build_plan<F: FnMut(BuildPlan) -> Result<(), anyhow::Error>>(
    build_dir: PathBuf,
    mut f: F,
) -> Result<(), anyhow::Error> {
    use std::io::Write;
    let build_dir = Utf8PathBuf::from_path_buf(build_dir)
        .map_err(|e| anyhow::format_err!("{:?} is not a utf8 path", e))?;

    let mut cmd = std::process::Command::new("cargo");
    if let Ok(dir) = std::env::current_dir() {
        cmd.current_dir(dir);
    }
    cmd.arg("-Z");
    cmd.arg("unstable-options");
    cmd.arg("build");
    cmd.arg("--build-plan");
    std::env::args().skip(2).for_each(|arg| {
        cmd.arg(arg);
    });
    cmd.envs(std::env::vars());
    cmd.env("CARGO_TARGET_DIR", build_dir.as_str());
    let output = cmd.output().expect("failed to execute process");

    if output.status.success() {
        let output = String::from_utf8(output.stdout.clone())?;
        let output = output
            .replace(build_dir.join("debug").as_str(), build_dir.as_str())
            .replace(build_dir.join("release").as_str(), build_dir.as_str());
        let plan = BuildPlan::from_cargo_output(&output.into_bytes())?;
        f(plan)?;
    }
    std::io::stderr().write_all(&output.stderr)?;
    Ok(())
}
