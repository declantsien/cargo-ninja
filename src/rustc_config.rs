use camino::Utf8PathBuf;
use clap::arg;
use clap::ArgAction;

pub fn rustc() -> clap::Command {
    clap::Command::new("rustc")
        // .ignore_errors(true)
        .arg(
            arg!(<INPUT> "source")
                .value_parser(clap::value_parser!(Utf8PathBuf)),
        )
        .arg(arg!(--cfg <SPEC>
            "Configure the compilation environment.
SPEC supports the syntax `NAME[=\"VALUE\"]`.)")
            .action(ArgAction::Append))
        .arg(arg!(--"check-cfg" <SPEC>
            "Provide list of valid cfg options for checking")
            .action(ArgAction::Append))
        .arg(arg!(-L <"[KIND=]PATH">
            "Add a directory to the library search path. The
optional KIND can be one of dependency, crate, native,
framework, or all (the default).")
            .action(ArgAction::Append))
        .arg(arg!(-l <"[KIND[:MODIFIERS]=]NAME[:RENAME]">
            "Link the generated crate(s) to the specified native
library NAME. The optional KIND can be one of
static, framework, or dylib (the default).
Optional comma separated MODIFIERS
(bundle|verbatim|whole-archive|as-needed)
may be specified each with a prefix of either '+' to
enable or '-' to disable.")
            .action(ArgAction::Append))
        .arg(arg!(--"crate-type" <TYPE>
            "Comma separated list of types
(bin|lib|rlib|dylib|cdylib|staticlib|proc-macro)
of crates for the compiler to emit"))
        .arg(arg!(--"crate-name" <NAME>
            "Specify the name of the crate being built"))
        .arg(arg!(--edition <EDITION>
            "Specify which edition of the compiler (2015|2018|2021|2024)
to use when compiling code."))
        .arg(arg!(--emit <"TYPE[,TYPE]">
            "Comma separated list of types
(asm|llvm-bc|llvm-ir|obj|metadata|link|dep-info|mir)
of output for the compiler to emit"))
        .arg(arg!(--print <INFO>
            "Compiler information to print on stdout
[crate-name|file-names|sysroot|target-libdir|cfg|calling-conventions|target-list|target-cpus|target-features|relocation-models|code-models|tls-models|target-spec-json|native-static-libs|stack-protector-strategies|link-args]"))
        .arg(arg!(debug: -g "Equivalent to -C debuginfo=2"))
        .arg(arg!(opt: -O "Equivalent to -C opt-level=2"))
        .arg(arg!(-o <FILENAME> "Write output to FILENAME"))
        .arg(arg!(--"out-dir" <DIR> "Write output to compiler-chosen filename in DIR"))
        .arg(arg!(--explain <OPT>   "Provide a detailed explanation of an error message"))
        .arg(arg!(--test "Build a test harness"))
        .arg(arg!(--target <TARGET> "Target triple for which the code is compiled"))
        .arg(arg!(-A --allow <LINT>    "Set lint allowed"))
        .arg(arg!(-W --warn <LINT>     "Set lint warnings"))
        .arg(arg!(--"force-warn" <LINT> "Set lint force-warn"))
        .arg(arg!(-D --deny <LINT>     "Set lint denied --target <TARGET>"))
        .arg(arg!(-F --forbid <LINT>   "Set lint forbidden"))
        .arg(arg!(--"cap-lints" <LEVEL>
                        "Set the most restrictive lint level. More restrictive
                        lints are capped at this level "))
        .arg(arg!(-C --codegen <"OPT[=VALUE]">
            "Set a codegen option")
            .action(ArgAction::Append))
        .arg(arg!(--extern <"NAME[=PATH]">
            "Specify where an external rust library is located")
            .action(ArgAction::Append))
        .arg(arg!(--sysroot <PATH>
            "Override the system root"))
        .arg(arg!(-Z <FLAG> "Set unstable / perma-unstable options")
            .action(ArgAction::Append))
        .arg(arg!(--"error-format" <FORMAT>
            "How (human|json|short) errors and other messages are produced"))
        .arg(arg!(--json <CONFIG> "Configure the JSON output of the compiler")
            .action(ArgAction::Append))
        .arg(arg!(--color <COLOR>
            "Configure coloring of output:
auto   = colorize, if output goes to a tty (default);
always = always colorize output;
never  = never colorize output"))
        .arg(arg!(--"diagnostic-width" <WIDTH>
            "Inform rustc of the width of the output so that diagnostics can be truncated to fit"))
        .arg(arg!(--"remap-path-prefix" <"FROM=TO">
            "Remap source names in all output (compiler messages and output files)")
            .action(ArgAction::Append))
        .arg(arg!(--"env-set" <"VAR=VALUE"> "Inject an environment variable")
            .action(ArgAction::Append))
        .arg(arg!(-v --verbose "Use verbose output"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let matches = rustc().get_matches_from(["rustc", "lib.rs"]);
        let input = matches.get_one::<Utf8PathBuf>("INPUT");
        assert_eq!(input, Some(&Utf8PathBuf::from("lib.rs")));
    }

    #[test]
    fn test() {
        let args = ["rustc", "--crate-name", "cargo_ninja", "--edition=2021", "src/main.rs", "--crate-type", "bin", "--emit=dep-info,link", "-C", "embed-bitcode=no", "-C", "debuginfo=2", "-C", "metadata=040056ab44031190", "-C", "extra-filename=-040056ab44031190", "--out-dir", "/home/declan/src/cargo-ninja/builddir/deps", "-C", "incremental=/home/declan/src/cargo-ninja/builddir/incremental", "-L", "dependency=/home/declan/src/cargo-ninja/builddir/deps", "--extern", "anyhow=/home/declan/src/cargo-ninja/builddir/deps/libanyhow-a0fdca5964864e0f.rlib", "--extern", "camino=/home/declan/src/cargo-ninja/builddir/deps/libcamino-a476909115397406.rlib", "--extern", "cargo_util=/home/declan/src/cargo-ninja/builddir/deps/libcargo_util-f63b173d29067151.rlib", "--extern", "cargo_util_schemas=/home/declan/src/cargo-ninja/builddir/deps/libcargo_util_schemas-3a505a01b7568eec.rlib", "--extern", "cargo_metadata=/home/declan/src/cargo-ninja/builddir/deps/libcargo_metadata-9e2c4e2b66a5a93a.rlib", "--extern", "clap=/home/declan/src/cargo-ninja/builddir/deps/libclap-796664f02e83d62c.rlib", "--extern", "ninja_files_data=/home/declan/src/cargo-ninja/builddir/deps/libninja_files_data2-4d3340732c142be6.rlib", "--extern", "ninja_files=/home/declan/src/cargo-ninja/builddir/deps/libninja_files2-f66972fdbb663726.rlib", "--extern", "pathdiff=/home/declan/src/cargo-ninja/builddir/deps/libpathdiff-602708d6b396de84.rlib", "--extern", "serde=/home/declan/src/cargo-ninja/builddir/deps/libserde-b3e3479ed1a980e0.rlib", "--extern", "serde_derive=/home/declan/src/cargo-ninja/builddir/deps/libserde_derive-badbf5fd040a4378.so", "--extern", "serde_json=/home/declan/src/cargo-ninja/builddir/deps/libserde_json-e1fa0a3f8528d24e.rlib", "--extern", "snailquote=/home/declan/src/cargo-ninja/builddir/deps/libsnailquote-8a178f26917bb5a0.rlib", "--error-format=human"];
        let matches = rustc().get_matches_from(args);
        let input = matches.get_one::<Utf8PathBuf>("INPUT");
        assert_eq!(input, Some(&Utf8PathBuf::from("src/main.rs")));
    }
}
