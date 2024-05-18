//! How to execute a build script and parse its output.
//!
//! ## Running a build script
//!
//! When running a build script, Cargo is aware of the progress and the result
//! of a build script. Standard output is the chosen interprocess communication
//! between Cargo and build script processes. A set of strings is defined for
//! that purpose. These strings, a.k.a. instructions, are interpreted by
//! [`BuildOutput::parse`] and stored in [`BuildRunner::build_script_outputs`].
//! The entire execution work is constructed by [`build_work`].
//!
//! [build script]: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html
//! [`TargetKind::CustomBuild`]: crate::core::manifest::TargetKind::CustomBuild
//! [`UnitGraph`]: super::unit_graph::UnitGraph
//! [`CompileMode::RunCustomBuild`]: super::CompileMode
//! [instructions]: https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script

use anyhow::bail;
use cargo_util::paths;
use cargo_util_schemas::manifest::RustVersion;
use ninja_files_data::CommandBuilder;
use snailquote::escape;
use std::path::{Path, PathBuf};
use std::str::{self, FromStr};

use crate::build_plan::Invocation;

/// Contains the parsed output of a custom build script.
#[derive(Clone, Debug, Hash, Default)]
pub struct BuildScriptOutput {
    /// Paths to pass to rustc with the `-L` flag.
    pub library_paths: Vec<PathBuf>,
    /// Names and link kinds of libraries, suitable for the `-l` flag.
    pub library_links: Vec<String>,
    /// Linker arguments suitable to be passed to `-C link-arg=<args>`
    pub linker_args: Vec<(LinkArgTarget, String)>,
    /// Various `--cfg` flags to pass to the compiler.
    pub cfgs: Vec<String>,
    /// Various `--check-cfg` flags to pass to the compiler.
    pub check_cfgs: Vec<String>,
    /// Additional environment variables to run the compiler with.
    pub env: Vec<(String, String)>,
    /// Metadata to pass to the immediate dependencies.
    pub metadata: Vec<(String, String)>,
    /// Paths to trigger a rerun of this build script.
    /// May be absolute or relative paths (relative to package root).
    pub rerun_if_changed: Vec<PathBuf>,
    /// Environment variables which, when changed, will cause a rebuild.
    pub rerun_if_env_changed: Vec<String>,
    /// Warnings generated by this build.
    ///
    /// These are only displayed if this is a "local" package, `-vv` is used,
    /// or there is a build error for any target in this package.
    pub warnings: Vec<String>,
}

/// Dependency information as declared by a build script that might trigger
/// a recompile of itself.
#[derive(Debug)]
pub struct BuildDeps {
    /// Absolute path to the file in the target directory that stores the
    /// output of the build script.
    pub build_script_output: PathBuf,
    /// Files that trigger a rebuild if they change.
    pub rerun_if_changed: Vec<PathBuf>,
    /// Environment variables that trigger a rebuild if they change.
    pub rerun_if_env_changed: Vec<String>,
}

/// Represents one of the instructions from `cargo::rustc-link-arg-*` build
/// script instruction family.
///
/// In other words, indicates targets that custom linker arguments applies to.
///
/// See the [build script documentation][1] for more.
///
/// [1]: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#cargorustc-link-argflag
#[derive(Clone, Hash, Debug, PartialEq, Eq)]
pub enum LinkArgTarget {
    /// Represents `cargo::rustc-link-arg=FLAG`.
    All,
    /// Represents `cargo::rustc-cdylib-link-arg=FLAG`.
    Cdylib,
    /// Represents `cargo::rustc-link-arg-bins=FLAG`.
    Bin,
    /// Represents `cargo::rustc-link-arg-bin=BIN=FLAG`.
    SingleBin(String),
    /// Represents `cargo::rustc-link-arg-tests=FLAG`.
    Test,
    /// Represents `cargo::rustc-link-arg-benches=FLAG`.
    Bench,
    /// Represents `cargo::rustc-link-arg-examples=FLAG`.
    Example,
}

impl LinkArgTarget {
    /// Checks if this link type applies to a given [`Target`].
    pub fn applies_to(&self, target: &Invocation) -> bool {
        match self {
            LinkArgTarget::All => true,
            LinkArgTarget::Cdylib => target.is_cdylib(),
            LinkArgTarget::Bin => target.is_bin(),
            LinkArgTarget::SingleBin(name) => target.is_bin() && target.package_name() == name,
            LinkArgTarget::Test => target.is_test(),
            LinkArgTarget::Bench => target.is_bench(),
            LinkArgTarget::Example => target.is_exe_example(),
        }
    }
}

impl BuildScriptOutput {
    /// Like [`BuildOutput::parse`] but from a file path.
    pub fn parse_file(
        path: &Path,
        library_name: Option<String>,
        pkg_descr: &str,
        script_out_dir_when_generated: &Path,
        script_out_dir: &Path,
        extra_check_cfg: bool,
        nightly_features_allowed: bool,
        msrv: &Option<RustVersion>,
    ) -> anyhow::Result<BuildScriptOutput> {
        let contents = paths::read_bytes(path)?;
        BuildScriptOutput::parse(
            &contents,
            library_name,
            pkg_descr,
            script_out_dir_when_generated,
            script_out_dir,
            extra_check_cfg,
            nightly_features_allowed,
            msrv,
        )
    }

    /// Parses the output instructions of a build script.
    ///
    /// * `pkg_descr` --- for error messages
    /// * `library_name` --- for determining if `RUSTC_BOOTSTRAP` should be allowed
    /// * `extra_check_cfg` --- for unstable feature [`-Zcheck-cfg`]
    ///
    /// [`-Zcheck-cfg`]: https://doc.rust-lang.org/cargo/reference/unstable.html#check-cfg
    pub fn parse(
        input: &[u8],
        // Takes String instead of InternedString so passing `unit.pkg.name()` will give a compile error.
        library_name: Option<String>,
        pkg_descr: &str,
        script_out_dir_when_generated: &Path,
        script_out_dir: &Path,
        extra_check_cfg: bool,
        nightly_features_allowed: bool,
        msrv: &Option<RustVersion>,
    ) -> anyhow::Result<BuildScriptOutput> {
        let mut library_paths = Vec::new();
        let mut library_links = Vec::new();
        let mut linker_args = Vec::new();
        let mut cfgs = Vec::new();
        let mut check_cfgs = Vec::new();
        let mut env = Vec::new();
        let mut metadata = Vec::new();
        let mut rerun_if_changed = Vec::new();
        let mut rerun_if_env_changed = Vec::new();
        let mut warnings = Vec::new();
        let whence = format!("build script of `{}`", pkg_descr);
        // Old syntax:
        //    cargo:rustc-flags=VALUE
        //    cargo:KEY=VALUE (for other unreserved keys)
        // New syntax:
        //    cargo::rustc-flags=VALUE
        //    cargo::metadata=KEY=VALUE (for other unreserved keys)
        // Due to backwards compatibility, no new keys can be added to this old format.
        const RESERVED_PREFIXES: &[&str] = &[
            "rustc-flags=",
            "rustc-link-lib=",
            "rustc-link-search=",
            "rustc-link-arg-cdylib=",
            "rustc-cdylib-link-arg=",
            "rustc-link-arg-bins=",
            "rustc-link-arg-bin=",
            "rustc-link-arg-tests=",
            "rustc-link-arg-benches=",
            "rustc-link-arg-examples=",
            "rustc-link-arg=",
            "rustc-cfg=",
            "rustc-check-cfg=",
            "rustc-env=",
            "warning=",
            "rerun-if-changed=",
            "rerun-if-env-changed=",
        ];
        const DOCS_LINK_SUGGESTION: &str = "See https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script \
                for more information about build script outputs.";

        fn check_minimum_supported_rust_version_for_new_syntax(
            pkg_descr: &str,
            msrv: &Option<RustVersion>,
        ) -> anyhow::Result<()> {
            let new_syntax_added_in = &RustVersion::from_str("1.77.0")?;

            if let Some(msrv) = msrv {
                if msrv < new_syntax_added_in {
                    bail!(
                        "the `cargo::` syntax for build script output instructions was added in \
                        Rust 1.77.0, but the minimum supported Rust version of `{pkg_descr}` is {msrv}.\n\
                        {DOCS_LINK_SUGGESTION}"
                    );
                }
            }

            Ok(())
        }

        fn parse_directive<'a>(
            whence: &str,
            line: &str,
            data: &'a str,
            old_syntax: bool,
        ) -> anyhow::Result<(&'a str, &'a str)> {
            let mut iter = data.splitn(2, "=");
            let key = iter.next();
            let value = iter.next();
            match (key, value) {
                (Some(a), Some(b)) => Ok((a, b.trim_end())),
                _ => bail!(
                    "invalid output in {whence}: `{line}`\n\
                    Expected a line with `{syntax}KEY=VALUE` with an `=` character, \
                    but none was found.\n\
                    {DOCS_LINK_SUGGESTION}",
                    syntax = if old_syntax { "cargo:" } else { "cargo::" },
                ),
            }
        }

        fn parse_metadata<'a>(
            whence: &str,
            line: &str,
            data: &'a str,
            old_syntax: bool,
        ) -> anyhow::Result<(&'a str, &'a str)> {
            let mut iter = data.splitn(2, "=");
            let key = iter.next();
            let value = iter.next();
            match (key, value) {
                (Some(a), Some(b)) => Ok((a, b.trim_end())),
                _ => bail!(
                    "invalid output in {whence}: `{line}`\n\
                    Expected a line with `{syntax}KEY=VALUE` with an `=` character, \
                    but none was found.\n\
                    {DOCS_LINK_SUGGESTION}",
                    syntax = if old_syntax {
                        "cargo:"
                    } else {
                        "cargo::metadata="
                    },
                ),
            }
        }

        for line in input.split(|b| *b == b'\n') {
            let line = match str::from_utf8(line) {
                Ok(line) => line.trim(),
                Err(..) => continue,
            };
            let mut old_syntax = false;
            let (key, value) = if let Some(data) = line.strip_prefix("cargo::") {
                check_minimum_supported_rust_version_for_new_syntax(pkg_descr, msrv)?;
                // For instance, `cargo::rustc-flags=foo` or `cargo::metadata=foo=bar`.
                parse_directive(whence.as_str(), line, data, old_syntax)?
            } else if let Some(data) = line.strip_prefix("cargo:") {
                old_syntax = true;
                // For instance, `cargo:rustc-flags=foo`.
                if RESERVED_PREFIXES
                    .iter()
                    .any(|prefix| data.starts_with(prefix))
                {
                    parse_directive(whence.as_str(), line, data, old_syntax)?
                } else {
                    // For instance, `cargo:foo=bar`.
                    ("metadata", data)
                }
            } else {
                // Skip this line since it doesn't start with "cargo:" or "cargo::".
                continue;
            };
            // This will rewrite paths if the target directory has been moved.
            let value = value.replace(
                script_out_dir_when_generated.to_str().unwrap(),
                script_out_dir.to_str().unwrap(),
            );

            let syntax_prefix = if old_syntax { "cargo:" } else { "cargo::" };
            macro_rules! add_target {
                ($link_type: expr) => {
                    linker_args.push(($link_type, value));
                };
            }

            // Keep in sync with TargetConfig::parse_links_overrides.
            match key {
                "rustc-flags" => {
                    let (paths, links) = BuildScriptOutput::parse_rustc_flags(&value, &whence)?;
                    library_links.extend(links.into_iter());
                    library_paths.extend(paths.into_iter());
                }
                "rustc-link-lib" => library_links.push(value.to_string()),
                "rustc-link-search" => library_paths.push(PathBuf::from(value)),
                "rustc-link-arg-cdylib" | "rustc-cdylib-link-arg" => {
                    linker_args.push((LinkArgTarget::Cdylib, value))
                }
                "rustc-link-arg-bins" => {
                    add_target!(LinkArgTarget::Bin);
                }
                "rustc-link-arg-bin" => {
                    let (bin_name, arg) = value.split_once('=').ok_or_else(|| {
                        anyhow::format_err!(
                            "invalid instruction `{}{}={}` from {}\n\
                                The instruction should have the form {}{}=BIN=ARG",
                            syntax_prefix,
                            key,
                            value,
                            whence,
                            syntax_prefix,
                            key
                        )
                    })?;
                    linker_args.push((
                        LinkArgTarget::SingleBin(bin_name.to_owned()),
                        arg.to_string(),
                    ));
                }
                "rustc-link-arg-tests" => {
                    add_target!(LinkArgTarget::Test);
                }
                "rustc-link-arg-benches" => {
                    add_target!(LinkArgTarget::Bench);
                }
                "rustc-link-arg-examples" => {
                    add_target!(LinkArgTarget::Example);
                }
                "rustc-link-arg" => {
                    linker_args.push((LinkArgTarget::All, value));
                }
                "rustc-cfg" => cfgs.push(value.to_string()),
                "rustc-check-cfg" => {
                    if extra_check_cfg {
                        check_cfgs.push(value.to_string());
                    } else {
                        // silently ignoring the instruction to try to
                        // minimise MSRV annoyance when stabilizing -Zcheck-cfg
                    }
                }
                "rustc-env" => {
                    let (key, val) = BuildScriptOutput::parse_rustc_env(&value, &whence)?;
                    // Build scripts aren't allowed to set RUSTC_BOOTSTRAP.
                    // See https://github.com/rust-lang/cargo/issues/7088.
                    if key == "RUSTC_BOOTSTRAP" {
                        // If RUSTC_BOOTSTRAP is already set, the user of Cargo knows about
                        // bootstrap and still wants to override the channel. Give them a way to do
                        // so, but still emit a warning that the current crate shouldn't be trying
                        // to set RUSTC_BOOTSTRAP.
                        // If this is a nightly build, setting RUSTC_BOOTSTRAP wouldn't affect the
                        // behavior, so still only give a warning.
                        // NOTE: cargo only allows nightly features on RUSTC_BOOTSTRAP=1, but we
                        // want setting any value of RUSTC_BOOTSTRAP to downgrade this to a warning
                        // (so that `RUSTC_BOOTSTRAP=library_name` will work)
                        let rustc_bootstrap_allows = |name: Option<&str>| {
                            let name = match name {
                                // as of 2021, no binaries on crates.io use RUSTC_BOOTSTRAP, so
                                // fine-grained opt-outs aren't needed. end-users can always use
                                // RUSTC_BOOTSTRAP=1 from the top-level if it's really a problem.
                                None => return false,
                                Some(n) => n,
                            };
                            // ALLOWED: the process of rustc bootstrapping reads this through
                            // `std::env`. We should make the behavior consistent. Also, we
                            // don't advertise this for bypassing nightly.
                            #[allow(clippy::disallowed_methods)]
                            std::env::var("RUSTC_BOOTSTRAP")
                                .map_or(false, |var| var.split(',').any(|s| s == name))
                        };
                        if nightly_features_allowed
                            || rustc_bootstrap_allows(library_name.as_deref())
                        {
                            warnings.push(format!("Cannot set `RUSTC_BOOTSTRAP={}` from {}.\n\
                                note: Crates cannot set `RUSTC_BOOTSTRAP` themselves, as doing so would subvert the stability guarantees of Rust for your project.",
                                val, whence
                            ));
                        } else {
                            // Setting RUSTC_BOOTSTRAP would change the behavior of the crate.
                            // Abort with an error.
                            bail!("Cannot set `RUSTC_BOOTSTRAP={}` from {}.\n\
                                note: Crates cannot set `RUSTC_BOOTSTRAP` themselves, as doing so would subvert the stability guarantees of Rust for your project.\n\
                                help: If you're sure you want to do this in your project, set the environment variable `RUSTC_BOOTSTRAP={}` before running cargo instead.",
                                val,
                                whence,
                                library_name.as_deref().unwrap_or("1"),
                            );
                        }
                    } else {
                        env.push((key, val));
                    }
                }
                "warning" => warnings.push(value.to_string()),
                "rerun-if-changed" => rerun_if_changed.push(PathBuf::from(value)),
                "rerun-if-env-changed" => rerun_if_env_changed.push(value.to_string()),
                "metadata" => {
                    let (key, value) = parse_metadata(whence.as_str(), line, &value, old_syntax)?;
                    metadata.push((key.to_owned(), value.to_owned()));
                }
                _ => bail!(
                    "invalid output in {whence}: `{line}`\n\
                    Unknown key: `{key}`.\n\
                    {DOCS_LINK_SUGGESTION}",
                ),
            }
        }

        Ok(BuildScriptOutput {
            library_paths,
            library_links,
            linker_args,
            cfgs,
            check_cfgs,
            env,
            metadata,
            rerun_if_changed,
            rerun_if_env_changed,
            warnings,
        })
    }

    /// Parses [`cargo::rustc-flags`] instruction.
    ///
    /// [`cargo::rustc-flags`]: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#cargorustc-flagsflags
    pub fn parse_rustc_flags(
        value: &str,
        whence: &str,
    ) -> anyhow::Result<(Vec<PathBuf>, Vec<String>)> {
        let value = value.trim();
        let mut flags_iter = value
            .split(|c: char| c.is_whitespace())
            .filter(|w| w.chars().any(|c| !c.is_whitespace()));
        let (mut library_paths, mut library_links) = (Vec::new(), Vec::new());

        while let Some(flag) = flags_iter.next() {
            if flag.starts_with("-l") || flag.starts_with("-L") {
                // Check if this flag has no space before the value as is
                // common with tools like pkg-config
                // e.g. -L/some/dir/local/lib or -licui18n
                let (flag, mut value) = flag.split_at(2);
                if value.is_empty() {
                    value = match flags_iter.next() {
                        Some(v) => v,
                        None => bail! {
                            "Flag in rustc-flags has no value in {}: {}",
                            whence,
                            value
                        },
                    }
                }

                match flag {
                    "-l" => library_links.push(value.to_string()),
                    "-L" => library_paths.push(PathBuf::from(value)),

                    // This was already checked above
                    _ => unreachable!(),
                };
            } else {
                bail!(
                    "Only `-l` and `-L` flags are allowed in {}: `{}`",
                    whence,
                    value
                )
            }
        }
        Ok((library_paths, library_links))
    }

    /// Parses [`cargo::rustc-env`] instruction.
    ///
    /// [`cargo::rustc-env`]: https://doc.rust-lang.org/nightly/cargo/reference/build-scripts.html#rustc-env
    pub fn parse_rustc_env(value: &str, whence: &str) -> anyhow::Result<(String, String)> {
        match value.split_once('=') {
            Some((n, v)) => Ok((n.to_owned(), v.to_owned())),
            _ => bail!("Variable rustc-env has no value in {whence}: {value}"),
        }
    }
}

#[allow(dead_code)]
impl BuildDeps {
    /// Creates a build script dependency information from a previous
    /// build script output path and the content.
    pub fn new(output_file: &Path, output: Option<&BuildScriptOutput>) -> BuildDeps {
        BuildDeps {
            build_script_output: output_file.to_path_buf(),
            rerun_if_changed: output
                .map(|p| &p.rerun_if_changed)
                .cloned()
                .unwrap_or_default(),
            rerun_if_env_changed: output
                .map(|p| &p.rerun_if_env_changed)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

/// Adds extra rustc flags and environment variables collected from the output
/// of a build-script to the command to execute, include custom environment
/// variables and `cfg`.
pub fn add_custom_flags(
    cmd: CommandBuilder,
    output: Option<&BuildScriptOutput>,
    package_name: &str,
    target: &Invocation,
) -> CommandBuilder {
    if output.is_none() {
        return cmd;
    }
    let output = output.unwrap();

    let cmd = output.cfgs.iter().fold(cmd, |cmd, cfg| {
        cmd.arg("--cfg").arg(escape(cfg.as_str()).into_owned())
    });

    let cmd = output
        .check_cfgs
        .iter()
        .enumerate()
        .fold(cmd, |mut cmd, (i, cfg)| {
            if i == 0 {
                cmd = cmd.arg("-Zunstable-options");
            }
            cmd.arg("--check-cfg")
                .arg(escape(cfg.as_str()).into_owned())
        });

    let cmd = output
        .env
        .iter()
        .fold(cmd, |cmd, (name, value)| cmd.env(name, value));

    let mut cmd = output.library_paths.iter().fold(cmd, |cmd, path| {
        cmd.arg("-L").arg(path.to_string_lossy().into_owned())
    });

    let pass_l_flag = target.is_lib();
    if pass_l_flag {
        cmd = output
            .library_links
            .iter()
            .fold(cmd, |cmd, name| cmd.arg("-l").arg(name.as_str()));
    }

    let cmd = output.linker_args.iter().fold(cmd, |cmd, (lt, arg)| {
        // There was an unintentional change where cdylibs were
        // allowed to be passed via transitive dependencies. This
        // clause should have been kept in the `if` block above. For
        // now, continue allowing it for cdylib only.
        // See https://github.com/rust-lang/cargo/issues/9562
        if lt.applies_to(target) && *lt == LinkArgTarget::Cdylib {
            return cmd.arg("-C").arg(format!("link-arg={}", arg));
        }
        cmd
    });

    output.metadata.iter().fold(cmd, |cmd, (key, value)| {
        cmd.env(
            &format!("DEP_{}_{}", envify(package_name), envify(key)),
            value,
        )
    })
}

fn envify(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_uppercase())
        .map(|c| if c == '-' { '_' } else { c })
        .collect()
}
