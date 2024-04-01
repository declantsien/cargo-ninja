#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod build_plan;

use build_plan::{BuildPlan, Invocation};
use ninja_files::format::write_ninja_file;
use ninja_files_data::{BuildBuilder, CommandBuilder, File, FileBuilder, RuleBuilder};
use ninja_writer::escape;
use std::{fs, path::PathBuf};
const BUILD_NINJA: &str = "build.ninja";
const LINK_RULE_ID: &str = "link";

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

    pub fn ninja_build(&self, indice: usize, deps: Vec<PathBuf>) -> FileBuilder {
        let rule_id = self.rule_id(indice);
        println!("{rule_id:?}");
        let rule = {
            let command = CommandBuilder::new(self.program.clone());
            let command = command.cwd(
                self.cwd
                    .as_ref()
                    .map(|cwd| cwd.to_str().expect("non utf8 path")),
            );

            let command = self.args.iter().fold(command, |cmd, arg| {
                cmd.arg(escape(format!("'{}'", arg).as_str()).into_owned())
            });
            let command = self.env.iter().fold(command, |cmd, env| {
                cmd.env(env.0.as_str(), escape(format!("\"{}\"", env.1).as_str()))
            });

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
            let utf8_path = o.to_str();
            if utf8_path.is_none() {
                println!("warning: non utf8 path {}", o.display());
                return builder;
            }
            println!("warning: utf8 path {}", o.display());
            builder.file(utf8_path.unwrap(), build.clone())
        });

        self.links().iter().fold(file, |builder, (link, target)| {
            let f = FileBuilder::new().rule(LINK_RULE_ID, link_rule());
            let build = BuildBuilder::new(LINK_RULE_ID);
            let build = build.explicit(target.to_str().expect("non utf-8 path"));
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
                let deps: Vec<PathBuf> = inv.deps.iter().fold(Vec::new(), |mut all_outputs, i| {
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
    let bytes = fs::read("/home/declan/src/cargo-build-plan/build-plan.json")?;
    let plan = BuildPlan::from_cargo_output(bytes)?;
    let ninja: File = plan.into();
    let file = std::fs::File::create(BUILD_NINJA).unwrap();
    let _ = write_ninja_file(&ninja, file).unwrap();

    Ok(())
}
