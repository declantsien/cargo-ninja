#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod build_plan;

use build_plan::{BuildPlan, TargetKind};
use std::fs;
use std::process::Command;

use crate::build_plan::Invocation;

fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    let bytes = fs::read("/home/declan/src/cargo-build-plan/build-plan.json")?;
    let plan = BuildPlan::from_cargo_output(bytes)?;

    let tasks: Vec<(String, String)> = plan
        .invocations
        .iter()
        .map(|i| (i.package_name.clone(), i.compile_mode.clone()))
        .collect();

    let target = plan
        .invocations
        .iter()
        .find(|i| {
            i.package_name == "emacsng"
                && i.target_kind == TargetKind::CustomBuild
                && i.compile_mode == "run-custom-build"
        })
        .unwrap();

    exec(target, &plan);

    Ok(())
}

fn exec(invocation: &Invocation, plan: &BuildPlan) {
    for i in invocation.deps.clone() {
        let d = plan.invocations.get(i).unwrap();
        // println!(
        //     "invo ({}, {:?}, {}) depends on {} {:?} {}",
        //     invocation.package_name,
        //     invocation.target_kind,
        //     invocation.compile_mode,
        //     d.package_name,
        //     d.target_kind,
        //     d.compile_mode
        // );
        exec(d, plan)
    }
    invocation.exec()
}
