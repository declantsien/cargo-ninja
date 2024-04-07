#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

#[path = "../build_plan.rs"]
mod build_plan;

use crate::build_plan::with_build_plan;
use build_plan::BuildPlan;
use std::fs;

use crate::build_plan::Invocation;

impl Invocation {
    pub fn exec(&self) {
        use std::io::{self, Write};
        use std::process::Command;
        for output in self.outputs().clone() {
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

        if output.status.success() {
            for (link, target) in self.links().clone() {
                if let Some(dir) = target.as_path().parent() {
                    fs::create_dir_all(dir).expect("failed to create dir");
                }
                // println!("{link:?} {original:?}");
                if link.exists() {
                    fs::remove_file(link.clone()).expect("failed to remove old link")
                }
                if target.exists() {
                    fs::hard_link(target, link).expect("failed to create link");
                    // Hard link a.txt to b.txt
                }
            }
        }
        io::stdout().write_all(&output.stdout).unwrap();
        io::stderr().write_all(&output.stderr).unwrap();
    }
}

pub fn main() -> Result<(), anyhow::Error> {
    with_build_plan(|plan| {
        let target = plan.invocations.iter().find(|i| {
            i.package_name == "cargo-ninja"
                && i.target_kind
                    .iter()
                    .find(|kind| kind.as_str() == "custom-build")
                    .is_some()
                && i.compile_mode == "run-custom-build"
        });

        if let Some(target) = target {
            exec(target, &plan);
        }
        Ok(())
    })?;

    Ok(())
}

fn exec(invocation: &Invocation, plan: &BuildPlan) {
    for i in invocation.deps.clone() {
        let d = plan.invocations.get(i).unwrap();
        exec(d, plan)
    }
    invocation.exec()
}
