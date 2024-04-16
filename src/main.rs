#![feature(lazy_cell)]

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod build_plan;
mod crate_type;
mod custom_build;

use build_plan::{build_dir, with_build_plan, Invocation};
use camino::Utf8PathBuf;
use custom_build::{add_custom_flags, BuildScriptOutput};
use ninja_files::format::write_ninja_file;
use ninja_files_data::{BuildBuilder, CommandBuilder, File, FileBuilder, RuleBuilder};
use snailquote::escape;
use std::collections::BTreeSet;

const BUILD_NINJA: &str = "build.ninja";
const CONFIGURE_NINJA: &str = "configure.ninja";
const LINK_RULE_ID: &str = "link";
const ENSURE_DIR_ALL_RULE_ID: &str = "ensure_dir_all";

fn link_rule() -> RuleBuilder {
    let command = if cfg!(target_family = "windows") {
        CommandBuilder::new("mklink")
            .arg("/h")
            .arg("$out")
            .arg("$in")
    } else if cfg!(target_family = "unix") {
        CommandBuilder::new("ln").arg("-f").arg("$in").arg("$out")
    } else {
        unimplemented!()
    };
    RuleBuilder::new(command)
}

fn ensure_dir_all_rule() -> RuleBuilder {
    let command = if cfg!(target_family = "windows") {
        unimplemented!()
    } else if cfg!(target_family = "unix") {
        // $ mkdir -p "$(dirname $FILE)" && touch "$FILE"
        CommandBuilder::new("mkdir")
            .arg("-p")
            .arg("$$(dirname $out)")
            .arg("&&")
            .arg("touch")
            .arg("$out")
    } else {
        unimplemented!()
    };
    RuleBuilder::new(command)
}

fn ninja_dir(p: &Utf8PathBuf) -> Option<Utf8PathBuf> {
    p.parent().map(|p| p.to_path_buf().join(".ninja_dir"))
}

impl Invocation {
    pub fn description(&self) -> String {
        format!(
            "{} target {} for {}@{}",
            self.compile_mode,
            self.target_kind.description(),
            self.package_name,
            self.package_version
        )
    }
    pub fn rule_id(&self, indice: usize) -> String {
        format!(
            "{}-{}-{}-{}-{}",
            indice,
            self.package_name,
            self.package_version,
            self.target_kind.description(),
            self.compile_mode
        )
    }

    pub fn dirs(&self) -> BTreeSet<Utf8PathBuf> {
        self.outputs()
            .iter()
            .map(|o| ninja_dir(o))
            .fold(BTreeSet::new(), |mut all, p| {
                if let Some(p) = p {
                    all.insert(p);
                }
                all
            })
    }

    pub fn ninja_build(
        &self,
        indice: usize,
        deps: Vec<Utf8PathBuf>,
        build_script_output: Option<BuildScriptOutput>,
    ) -> FileBuilder {
        let rule_id = self.rule_id(indice);
        let rule = {
            let command = CommandBuilder::new(self.program.clone());
            let command = command.cwd(self.cwd.clone());

            let command = self.args.iter().fold(command, |cmd, arg| {
                if arg == "--error-format=json" || arg.starts_with("--json=") {
                    return cmd;
                }
                cmd.arg(escape(arg.as_str()).into_owned())
            });
            let command = command.arg("--error-format=human");
            let command = self.env.iter().fold(command, |cmd, env| {
                cmd.env(env.0.as_str(), escape(env.1.as_str()))
            });
            let command = add_custom_flags(
                command,
                build_script_output.as_ref(),
                self.package_name.as_str(),
                self,
            );

            let command = match self.is_run_custom_build() {
                true => command
                    .arg(">")
                    .arg(self.build_script_output_file().unwrap().as_str()),
                _ => command,
            };

            RuleBuilder::new(command)
        };
        let build = BuildBuilder::new(rule_id.clone());
        let build = deps.iter().fold(build, |build, d| build.explicit(d));

        let build = build.variable("description", self.description());

        let file = FileBuilder::new().rule(rule_id.clone(), rule);
        let file = self.outputs().iter().fold(file, |builder, o| {
            let build = build.clone();
            let build = match ninja_dir(o) {
                Some(p) => build.implicit(p),
                _ => build,
            };
            builder.output(o, build)
        });

        let file = self.dirs().iter().fold(file, |builder, dir| {
            let f = FileBuilder::new().rule(ENSURE_DIR_ALL_RULE_ID, ensure_dir_all_rule());
            let build = BuildBuilder::new(ENSURE_DIR_ALL_RULE_ID);
            let f = f.output(dir, build);
            builder.merge(&f)
        });

        self.links().iter().fold(file, |builder, (link, target)| {
            let f = FileBuilder::new().rule(LINK_RULE_ID, link_rule());
            let build = BuildBuilder::new(LINK_RULE_ID);
            let build = build.explicit(target);
            let build = match ninja_dir(target) {
                Some(p) => build.implicit(p),
                _ => build,
            };
            let f = f.output(link, build);
            builder.merge(&f)
        })
    }
}

fn configure() -> Result<(), anyhow::Error> {
    let build_dir = build_dir()?;
    with_build_plan(|plan| {
        for i in &plan.invocations {
            if let Ok(out_dir) = i.out_dir() {
                std::fs::create_dir_all(out_dir)?;
            }
        }
        // let ninja: File = plan.into();
        let ninja: File = plan.to_ninja(true, |i| i.is_run_custom_build());
        let file = std::fs::File::create(build_dir.join(CONFIGURE_NINJA))?;
        write_ninja_file(&ninja, file)?;
        Ok(())
    })?;

    use std::io::{self, Write};
    use std::process::Command;

    let output = Command::new("ninja")
        .current_dir(build_dir)
        .arg("-f")
        .arg(CONFIGURE_NINJA)
        .output()
        .expect("failed to execute process");

    if output.status.success() {}
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();

    Ok(())
}

fn main() -> Result<(), anyhow::Error> {
    let _ = configure()?;
    let build_dir = build_dir()?;
    with_build_plan(|plan| {
        for i in &plan.invocations {
            if let Ok(out_dir) = i.out_dir() {
                std::fs::create_dir_all(out_dir)?;
            }
        }
        // let ninja: File = plan.into();
        let ninja: File = plan.to_ninja(false, |i| i.is_workspace_build());
        let file = std::fs::File::create(build_dir.join(BUILD_NINJA))?;
        write_ninja_file(&ninja, file)?;
        Ok(())
    })?;

    Ok(())
}
