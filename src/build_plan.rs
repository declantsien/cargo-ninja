//! A parser for Cargo's `--build-plan` output.
//!
//! The main type is [`BuildPlan`]. To parse Cargo's output into a `BuildPlan`, call
//! [`BuildPlan::from_cargo_output`].
//!
//! [`BuildPlan`]: struct.BuildPlan.html
//! [`BuildPlan::from_cargo_output`]: struct.BuildPlan.html#method.from_cargo_output

#![warn(missing_debug_implementations)]

use std::borrow::BorrowMut;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
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
    pub outputs: Vec<PathBuf>,
    /// Hardlinks of output files that should be placed.
    pub links: BTreeMap<PathBuf, PathBuf>,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl Invocation {
    pub fn links(&self) -> BTreeMap<PathBuf, PathBuf> {
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

    pub fn outputs(&self) -> Vec<PathBuf> {
        let outputs = if self.outputs.is_empty() {
            vec![PathBuf::from(self.hash_string())]
        } else {
            self.outputs
                .clone()
                .into_iter()
                .filter(|output| !output.extension().map_or(false, |e| e == "dwp"))
                .collect()
        };
        // outputs
        //     .into_iter()
        //     .filter(|o| !o.extension().map_or(false, |e| e == "rmeta"))
        //     .collect()
        outputs
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
