#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod build_plan;

use build_plan::{BuildPlan, Invocation};
use ninja_files::format::write_ninja_file;
use ninja_files_data::{BuildBuilder, CommandBuilder, File, FileBuilder, RuleBuilder};
// use ninja_writer::escape;
// use shell_escape::escape;
use snailquote::escape;
use std::{fs, path::PathBuf};
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

fn ninja_dir(p: &PathBuf) -> Option<PathBuf> {
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

    pub fn dirs(&self) -> Vec<PathBuf> {
        if self.compile_mode == "run-custom-build" {
            return Vec::new();
        }
        self.outputs()
            .iter()
            .map(|o| ninja_dir(o))
            .fold(Vec::new(), |mut all, p| {
                if let Some(p) = p {
                    all.push(p);
                }
                all
            })
    }

    pub fn ninja_build(&self, indice: usize, deps: Vec<PathBuf>) -> FileBuilder {
        let rule_id = self.rule_id(indice);
        println!("{rule_id:?}");
        let rule = {
            // let command = CommandBuilder::new(self.program.clone());
            let command = CommandBuilder::new("strace").arg(self.program.clone());
            let command = command.cwd(
                self.cwd
                    .as_ref()
                    .map(|cwd| cwd.to_str().expect("non utf8 path")),
            );

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
                    .arg("&&")
                    .arg("cd -")
                    .arg("&&")
                    .arg("touch")
                    .arg(self.outputs().get(0).unwrap().to_str().unwrap()),
                false => command,
            };

            RuleBuilder::new(command)
        };
        let build = BuildBuilder::new(rule_id.clone());
        // println!("deps: {deps:?}");
        let build = deps.iter().fold(build, |build, d| {
            let utf8_path = d.to_str();
            if utf8_path.is_none() {
                println!("warning: non utf8 path {}", d.display());
                return build;
            }
            build.explicit(utf8_path.unwrap())
        });

        let file = FileBuilder::new().rule(rule_id.clone(), rule);
        let file = self.outputs().iter().fold(file, |builder, o| {
            let build = build.clone();
            let build = match ninja_dir(o) {
                Some(p) => build.implicit(p.to_str().expect("non utf-8 path")),
                _ => build,
            };
            let utf8_path = o.to_str();
            if utf8_path.is_none() {
                println!("warning: non utf8 path {}", o.display());
                return builder;
            }
            println!("warning: utf8 path {}", o.display());
            builder.file(utf8_path.unwrap(), build)
        });

        let file = self.dirs().iter().fold(file, |builder, dir| {
            let f = FileBuilder::new().rule(ENSURE_DIR_ALL_RULE_ID, ensure_dir_all_rule());
            let build = BuildBuilder::new(ENSURE_DIR_ALL_RULE_ID);
            let f = f.file(dir.to_str().expect("non utf8 path"), build);
            builder.merge(&f)
        });

        self.links().iter().fold(file, |builder, (link, target)| {
            let f = FileBuilder::new().rule(LINK_RULE_ID, link_rule());
            let build = BuildBuilder::new(LINK_RULE_ID);
            let build = build.explicit(target.to_str().expect("non utf-8 path"));
            let build = match ninja_dir(target) {
                Some(p) => build.implicit(p.to_str().expect("non utf-8 path")),
                _ => build,
            };
            let f = f.file(link.to_str().expect("non utf8 path"), build);
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
                let deps: Vec<PathBuf> = Vec::new();

                let deps: Vec<PathBuf> = inv.deps.iter().fold(deps, |mut all_outputs, i| {
                    let mut outputs = self.invocations[*i].outputs();
                    all_outputs.append(&mut outputs);
                    let mut links: Vec<PathBuf> = self.invocations[*i]
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

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    use std::io::Write;

    // let command = CommandBuilder::new("cargo")
    //     .cwd(std::env::current_dir().ok().and_then(|p| p.into_os_string().into_string().ok()))
    //     .arg("build")
    //     .arg("-Z")
    //     .arg("unstable-options")
    //     .arg("--build-plan");
    // let command = std::env::args().fold(command, |cmd, arg| cmd.arg(arg));
    // let command = std::env::vars().fold(command, |cmd, (key, val)| cmd.env(key, val));

    let mut cmd = std::process::Command::new("cargo");
    if let Ok(dir) = std::env::current_dir() {
        cmd.current_dir(dir);
    }
    cmd.arg("-Z");
    cmd.arg("unstable-options");
    cmd.arg("build");
    cmd.arg("--build-plan");
    std::env::args().enumerate().for_each(|(i, arg)| {
        if i == 0 {
            return;
        }
        cmd.arg(arg);
    });
    cmd.envs(std::env::vars());
    println!("{:?}", std::env::vars());
    let output = cmd.output().expect("failed to execute process");

    if output.status.success() {
        // std::io::stdout().write_all(&output.stdout).unwrap();
        let plan = BuildPlan::from_cargo_output(&output.stdout)?;
        let ninja: File = plan.into();
        let file = std::fs::File::create(BUILD_NINJA).unwrap();
        let _ = write_ninja_file(&ninja, file).unwrap();
    }

    std::io::stderr().write_all(&output.stderr).unwrap();

    Ok(())
}
