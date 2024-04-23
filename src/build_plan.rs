//! A parser for Cargo's `--build-plan` output.
//!
//! The main type is [`BuildPlan`]. To parse Cargo's output into a `BuildPlan`, call
//! [`BuildPlan::from_cargo_output`].
//!
//! [`BuildPlan`]: struct.BuildPlan.html
//! [`BuildPlan::from_cargo_output`]: struct.BuildPlan.html#method.from_cargo_output

#![warn(missing_debug_implementations)]

use camino::Utf8PathBuf;
use cargo_metadata::Metadata;
use cargo_metadata::MetadataCommand;
use ninja_files::format::write_ninja_file;
use ninja_files_data::{File, FileBuilder};
use serde::de;
use serde::de::Error;
use std::fmt;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::string::ToString;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{LazyLock, OnceLock},
};

use crate::crate_type::CrateType;
use crate::custom_build::BuildScriptOutput;

static METADATA: LazyLock<Metadata> = LazyLock::new(|| match MetadataCommand::new().exec() {
    Ok(d) => d,
    Err(e) => panic!("Metadata Command failed: {e:?}"),
});

#[allow(dead_code)]
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetKind {
    Lib(Vec<CrateType>),
    Bin,
    Test,
    Bench,
    ExampleLib(Vec<CrateType>),
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
            ref kinds => Lib(kinds
                .iter()
                .cloned()
                .map(|kind| CrateType::from(&kind.to_string()))
                .collect()),
        })
    }
}

impl fmt::Debug for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::TargetKind::*;
        match *self {
            Lib(ref kinds) => kinds.fmt(f),
            Bin => "bin".fmt(f),
            ExampleBin | ExampleLib(_) => "example".fmt(f),
            Test => "test".fmt(f),
            CustomBuild => "custom-build".fmt(f),
            Bench => "bench".fmt(f),
        }
    }
}

#[allow(dead_code)]
impl TargetKind {
    pub fn description(&self) -> &'static str {
        match self {
            TargetKind::Lib(..) => "lib",
            TargetKind::Bin => "bin",
            TargetKind::Test => "integration-test",
            TargetKind::ExampleBin | TargetKind::ExampleLib(..) => "example",
            TargetKind::Bench => "bench",
            TargetKind::CustomBuild => "build-script",
        }
    }

    /// Returns whether production of this artifact requires the object files
    /// from dependencies to be available.
    ///
    /// This only returns `false` when all we're producing is an rlib, otherwise
    /// it will return `true`.
    pub fn requires_upstream_objects(&self) -> bool {
        match self {
            TargetKind::Lib(kinds) | TargetKind::ExampleLib(kinds) => {
                kinds.iter().any(|k| k.requires_upstream_objects())
            }
            _ => true,
        }
    }

    /// Returns the arguments suitable for `--crate-type` to pass to rustc.
    pub fn rustc_crate_types(&self) -> Vec<CrateType> {
        match self {
            TargetKind::Lib(kinds) | TargetKind::ExampleLib(kinds) => kinds.clone(),
            TargetKind::CustomBuild
            | TargetKind::Bench
            | TargetKind::Test
            | TargetKind::ExampleBin
            | TargetKind::Bin => vec![CrateType::Bin],
        }
    }
}

/// The general "mode" for what to do.
/// This is used for two purposes. The commands themselves pass this in to
/// `compile_ws` to tell it the general execution strategy. This influences
/// the default targets selected. The other use is in the `Unit` struct
/// to indicate what is being done with a specific target.
#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash, PartialOrd, Ord)]
pub enum CompileMode {
    /// A target being built for a test.
    Test,
    /// Building a target with `rustc` (lib or bin).
    Build,
    /// Building a target with `rustc` to emit `rmeta` metadata only. If
    /// `test` is true, then it is also compiled with `--test` to check it like
    /// a test.
    Check { test: bool },
    /// Used to indicate benchmarks should be built. This is not used in
    /// `Unit`, because it is essentially the same as `Test` (indicating
    /// `--test` should be passed to rustc) and by using `Test` instead it
    /// allows some de-duping of Units to occur.
    Bench,
    /// A target that will be documented with `rustdoc`.

    /// If `deps` is true, then it will also document all dependencies.
    /// if `json` is true, the documentation output is in json format.
    Doc { deps: bool, json: bool },
    /// A target that will be tested with `rustdoc`.
    Doctest,
    /// An example or library that will be scraped for function calls by `rustdoc`.
    Docscrape,
    /// A marker for Units that represent the execution of a `build.rs` script.
    RunCustomBuild,
}

impl fmt::Display for CompileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::CompileMode::*;
        let v = match *self {
            Test => "test",
            Build => "build",
            Check { .. } => "check",
            Bench => "bench",
            Doc { .. } => "doc",
            Doctest => "doctest",
            Docscrape => "docscrape",
            RunCustomBuild => "run-custom-build",
        };
        write!(f, "{}", v)
    }
}

impl<'de> de::Deserialize<'de> for CompileMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        use self::CompileMode::*;

        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "test" => Test,
            "build" => Build,
            "check" => Check { test: false },
            "bench" => Bench,
            "doc" => Doc {
                deps: false,
                json: false,
            },
            "doctest" => Doctest,
            "docscrape" => Docscrape,
            "run-custom-build" => RunCustomBuild,
            _ => panic!("unknow compile mode {}", raw),
        })
    }
}

/// A tool invocation.
#[derive(Debug, Deserialize, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Invocation {
    pub package_name: String,
    pub package_version: String,
    pub target_kind: TargetKind,
    pub compile_mode: CompileMode,
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

#[allow(dead_code)]
impl Invocation {
    fn hash_string(&self) -> String {
        let mut s = DefaultHasher::new();
        (*self).hash(&mut s);
        let hash = s.finish();
        hash.to_string()
    }

    pub fn is_run_custom_build(&self) -> bool {
        self.compile_mode == CompileMode::RunCustomBuild
    }

    pub fn is_workspace_build(&self) -> bool {
        let workspace_packages = METADATA.workspace_packages();

        workspace_packages
            .into_iter()
            .find(|p| {
                p.name == self.package_name
                    && p.version.to_string() == self.package_version
                    && !self.is_run_custom_build()
                    && !self.is_custom_build()
            })
            .is_some()
    }

    pub fn links(&self) -> BTreeMap<Utf8PathBuf, Utf8PathBuf> {
        let links = self.links.clone();
        links
            .into_iter()
            .filter(|(_, target)| !target.extension().map_or(false, |e| e == "dwp"))
            .collect()
    }

    pub fn out_dir(&self) -> anyhow::Result<Utf8PathBuf> {
        let dir = self
            .env
            .iter()
            .find(|(key, _)| key.as_str() == "OUT_DIR")
            .ok_or(anyhow::format_err!("OUT_DIR is not set. {:?}", self))?
            .1;
        Ok(Utf8PathBuf::from(dir))
    }

    pub fn build_script_output_file(&self) -> anyhow::Result<Utf8PathBuf> {
        Ok(self
            .out_dir()?
            .parent()
            .ok_or(anyhow::format_err!("failed get out_dir's parent"))?
            .join("output"))
    }

    pub fn build_script_output(&self) -> anyhow::Result<BuildScriptOutput> {
        let file = self.build_script_output_file()?;
        let file = file.into_std_path_buf();
        if !file.exists() {
            run_build_script(&self)?;
        }
        // We currently using the same out_dir for rus_custom_build and build
        let dir = &file
            .parent()
            .ok_or(anyhow::format_err!("failed to get output dir"))?;
        BuildScriptOutput::parse_file(
            file.as_path(),
            Some(self.package_name.clone()),
            &self.package_name,
            dir,
            dir,
            true,
            true,
            &None,
        )
    }

    pub fn outputs(&self) -> Vec<Utf8PathBuf> {
        let outputs = if self.compile_mode == CompileMode::RunCustomBuild {
            vec![self
                .build_script_output_file()
                .expect("out_dir should set for run-custom-build")]
        } else {
            self.outputs
                .clone()
                .into_iter()
                .filter(|output| !output.extension().map_or(false, |e| e == "dwp"))
                .collect()
        };
        outputs
    }

    fn kind(&self) -> &TargetKind {
        &self.target_kind
    }

    pub fn doctestable(&self) -> bool {
        match self.kind() {
            TargetKind::Lib(ref kinds) => kinds.iter().any(|k| {
                *k == CrateType::Rlib || *k == CrateType::Lib || *k == CrateType::ProcMacro
            }),
            _ => false,
        }
    }

    pub fn is_lib(&self) -> bool {
        matches!(self.kind(), TargetKind::Lib(_))
    }

    pub fn is_dylib(&self) -> bool {
        match self.kind() {
            TargetKind::Lib(libs) => libs.iter().any(|l| *l == CrateType::Dylib),
            _ => false,
        }
    }

    pub fn is_cdylib(&self) -> bool {
        match self.kind() {
            TargetKind::Lib(libs) => libs.iter().any(|l| *l == CrateType::Cdylib),
            _ => false,
        }
    }

    pub fn is_staticlib(&self) -> bool {
        match self.kind() {
            TargetKind::Lib(libs) => libs.iter().any(|l| *l == CrateType::Staticlib),
            _ => false,
        }
    }

    /// Returns whether this target produces an artifact which can be linked
    /// into a Rust crate.
    ///
    /// This only returns true for certain kinds of libraries.
    pub fn is_linkable(&self) -> bool {
        match self.kind() {
            TargetKind::Lib(kinds) => kinds.iter().any(|k| k.is_linkable()),
            _ => false,
        }
    }

    pub fn is_bin(&self) -> bool {
        *self.kind() == TargetKind::Bin
    }

    pub fn is_example(&self) -> bool {
        matches!(
            self.kind(),
            TargetKind::ExampleBin | TargetKind::ExampleLib(..)
        )
    }

    /// Returns `true` if it is a binary or executable example.
    /// NOTE: Tests are `false`!
    pub fn is_executable(&self) -> bool {
        self.is_bin() || self.is_exe_example()
    }

    /// Returns `true` if it is an executable example.
    pub fn is_exe_example(&self) -> bool {
        // Needed for --all-examples in contexts where only runnable examples make sense
        matches!(self.kind(), TargetKind::ExampleBin)
    }

    pub fn is_test(&self) -> bool {
        *self.kind() == TargetKind::Test
    }
    pub fn is_bench(&self) -> bool {
        *self.kind() == TargetKind::Bench
    }
    pub fn is_custom_build(&self) -> bool {
        *self.kind() == TargetKind::CustomBuild
    }

    /// Returns the arguments suitable for `--crate-type` to pass to rustc.
    pub fn rustc_crate_types(&self) -> Vec<CrateType> {
        self.kind().rustc_crate_types()
    }

    pub(crate) fn package_name(&self) -> &str {
        self.package_name.as_str()
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
    pub fn from_cargo_output() -> anyhow::Result<Self> {
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

        let build_dir = build_dir()?;
        cmd.env("CARGO_TARGET_DIR", build_dir.as_str());

        let output = cmd.output().expect("failed to execute process");

        if output.status.success() {
            let mut data = output.stdout;
            let output = String::from_utf8(data.clone())?;
            let output = output
                .replace(build_dir.join("debug").as_str(), build_dir.as_str())
                .replace(build_dir.join("release").as_str(), build_dir.as_str());
            data = output.into_bytes();
            // these dirs are created when invoke cargo build --build-plan
            let cargo_debug_dir = build_dir.join("debug");
            if cargo_debug_dir.exists() {
                std::fs::remove_dir_all(cargo_debug_dir)?;
            }
            let cargo_release_dir = build_dir.join("release");
            if cargo_release_dir.exists() {
                std::fs::remove_dir_all(cargo_release_dir)?;
            }

            let plan = serde_json::from_slice(data.as_ref())?;

            return Ok(plan);
        }
        Err(anyhow::format_err!(
            "Cmd {cmd:?} failed: {:?}",
            &output.stderr
        ))
    }

    pub fn to_ninja<Filter: Fn(&&Invocation) -> bool>(
        &self,
        include_custom_build: bool,
        filter: Filter,
    ) -> File {
        let include_builds: Vec<&Invocation> = self.invocations.iter().filter(filter).collect();
        let mut deps: BTreeSet<usize> = BTreeSet::new();
        for invocation in &include_builds {
            collect_deps_recursively(invocation, self, &mut deps, include_custom_build);
        }

        self.invocations
            .iter()
            .enumerate()
            .fold(FileBuilder::new(), |builder, (i, inv)| {
                if !include_builds.contains(&inv) && !deps.contains(&i) {
                    return builder;
                }
                let deps: Vec<Utf8PathBuf> = Vec::new();
                let mut custom_build_output: Option<BuildScriptOutput> = None;

                let deps: Vec<Utf8PathBuf> = inv.deps.iter().fold(deps, |mut all_outputs, i| {
                    let dep = &self.invocations[*i];
                    if !dep.is_run_custom_build() {
                        let mut outputs = dep.outputs();
                        all_outputs.append(&mut outputs);
                        let mut links: Vec<Utf8PathBuf> = self.invocations[*i]
                            .links()
                            .into_iter()
                            .map(|(link, _)| link)
                            .collect();
                        all_outputs.append(&mut links);
                    } else {
                        custom_build_output = dep
                            .build_script_output()
                            .map_err(|e| {
                                eprintln!("Custom build output error: {e:?}");
                            })
                            .ok();
                    }
                    all_outputs
                });
                builder.merge(&inv.ninja_build(i, deps, custom_build_output))
            })
            .build()
            .unwrap()
    }
}

pub fn with_build_plan<F: FnMut(&BuildPlan) -> Result<(), anyhow::Error>>(
    mut f: F,
) -> Result<(), anyhow::Error> {
    static BUILD_PLAN: OnceLock<BuildPlan> = OnceLock::new();
    let plan = BuildPlan::from_cargo_output()?;
    let plan = BUILD_PLAN.get_or_init(|| plan);
    f(plan)
}

fn collect_deps_recursively(
    invocation: &Invocation,
    plan: &BuildPlan,
    deps: &mut BTreeSet<usize>,
    include_custom_build: bool,
) {
    for i in invocation.deps.clone() {
        let d = plan.invocations.get(i).unwrap();
        if !include_custom_build && (d.is_run_custom_build() || d.is_custom_build()) {
            continue;
        }
        deps.insert(i);
        collect_deps_recursively(d, plan, deps, include_custom_build)
    }
}

pub fn build_dir() -> Result<Utf8PathBuf, anyhow::Error> {
    let args = std::env::args().skip(1).take(1).collect::<Vec<String>>();
    let build_dir = args
        .get(0)
        .ok_or(anyhow::format_err!("no build directory specified"))?;

    let build_dir = std::env::current_dir()?.join(build_dir);
    std::fs::create_dir_all(build_dir.clone())?;
    let build_dir = Utf8PathBuf::from_path_buf(build_dir)
        .map_err(|e| anyhow::format_err!("{:?} is not a utf8 path", e))?;
    Ok(build_dir)
}

fn run_build_script(inv: &Invocation) -> Result<(), anyhow::Error> {
    let build_dir = build_dir()?;
    let dir = build_dir.join(".run_build_script");
    std::fs::create_dir_all(dir.clone())?;
    let ninja_file = dir.join(inv.hash_string());
    if !ninja_file.exists() {
        with_build_plan(|plan| {
            for i in &plan.invocations {
                if let Ok(out_dir) = i.out_dir() {
                    std::fs::create_dir_all(out_dir)?;
                }
            }
            let ninja: File = plan.to_ninja(true, |i| i == &inv);
            let file = std::fs::File::create(ninja_file.clone())?;
            write_ninja_file(&ninja, file)?;
            Ok(())
        })?;
    }

    use std::io::{self, Write};
    use std::process::Command;

    let output = Command::new("ninja")
        .arg("-f")
        .arg(ninja_file)
        .output()
        .expect("failed to execute process");

    if output.status.success() {}
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();

    Ok(())
}
