#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod build_plan;

use build_plan::{with_build_plan, BuildPlan, Invocation};
use camino::Utf8PathBuf;
use ninja_files::format::write_ninja_file;
use ninja_files_data::{BuildBuilder, CommandBuilder, File, FileBuilder, RuleBuilder};
use snailquote::escape;
use std::collections::BTreeSet;

const BUILD_NINJA: &str = "build.ninja";
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
    pub fn rule_id(&self, indice: usize) -> String {
        format!(
            "{}-{}-{}-{}-{}",
            indice,
            self.package_name,
            self.package_version,
            self.target_kind.get(0).unwrap(),
            self.compile_mode
        )
    }

    pub fn dirs(&self) -> BTreeSet<Utf8PathBuf> {
        if self.compile_mode == "run-custom-build" {
            return BTreeSet::new();
        }
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

    pub fn ninja_build(&self, indice: usize, deps: Vec<Utf8PathBuf>) -> FileBuilder {
        let rule_id = self.rule_id(indice);
        let rule = {
            let command = CommandBuilder::new(self.program.clone());
            // let command = CommandBuilder::new("strace").arg(self.program.clone());
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

            let command = match self.compile_mode == "run-custom-build" {
                true => command
                    .arg(">")
                    .arg("$$OLDPWD/".to_string() + self.outputs().get(0).unwrap().as_str()),
                false => command,
            };

            RuleBuilder::new(command)
        };
        let build = BuildBuilder::new(rule_id.clone());
        // println!("deps: {deps:?}");
        let build = deps.iter().fold(build, |build, d| build.explicit(d));

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

impl Into<File> for BuildPlan {
    fn into(self) -> File {
        self.invocations
            .iter()
            .enumerate()
            .fold(FileBuilder::new(), |builder, (i, inv)| {
                let deps: Vec<Utf8PathBuf> = Vec::new();

                let deps: Vec<Utf8PathBuf> = inv.deps.iter().fold(deps, |mut all_outputs, i| {
                    let mut outputs = self.invocations[*i].outputs();
                    all_outputs.append(&mut outputs);
                    let mut links: Vec<Utf8PathBuf> = self.invocations[*i]
                        .links()
                        .into_iter()
                        .map(|(link, _)| link)
                        .collect();
                    all_outputs.append(&mut links);
                    all_outputs
                });
                builder.merge(&inv.ninja_build(i, deps))
            })
            .build()
            .unwrap()
    }
}

fn main() -> Result<(), anyhow::Error> {
    let args = std::env::args().skip(1).take(1).collect::<Vec<String>>();
    let build_dir = args
        .get(0)
        .ok_or(anyhow::format_err!("no build directory specified"))?;

    let build_dir = std::env::current_dir()?.join(build_dir);
    std::fs::create_dir_all(build_dir.clone())?;
    with_build_plan(build_dir.clone(), |plan| {
        for i in &plan.invocations {
            if let Some((_, output_dir)) = i.env.iter().find(|(key, _)| key.as_str() == "OUT_DIR") {
                std::fs::create_dir_all(Utf8PathBuf::from(output_dir))?;
            }
        }
        let ninja: File = plan.into();
        let file = std::fs::File::create(build_dir.join(BUILD_NINJA))?;
        write_ninja_file(&ninja, file)?;
        Ok(())
    })?;

    Ok(())
}
