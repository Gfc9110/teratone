use std::{env, path::PathBuf, process::Command};
fn run(command: &mut Command) {
    let description = format!("{command:?}");
    let status = command.status().unwrap_or_else(|e| panic!("{description}: {e}"));
    assert!(status.success(), "Failed: {description}");
}
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).parent().unwrap().to_owned();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("native");
    let mut configure = Command::new("cmake");
    configure.arg("-S").arg(&root).arg("-B").arg(&out)
        .arg("-DCMAKE_BUILD_TYPE=Release").arg("-DBUILD_TESTING=OFF");
    let target = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();
    assert_eq!(host, target, "Native desktop builds only in this increment; cross-build adapters are not implemented");
    if target.contains("msvc") {
        configure.arg("-A").arg(if target.starts_with("aarch64") { "ARM64" } else { "x64" });
    }
    run(&mut configure);
    run(Command::new("cmake").arg("--build").arg(&out).arg("--config").arg("Release")
        .arg("--target").arg("open_daw_api_static").arg("--parallel").arg("2"));
    println!("cargo:rustc-link-search=native={}", out.join("lib").display());
    for lib in ["open_daw_api_static", "open_daw_engine", "open_daw_core"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if target.contains("apple-darwin") {
        println!("cargo:rustc-link-lib=c++");
        for framework in ["AudioUnit", "AudioToolbox", "CoreAudio"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else if target.contains("windows") {
        for lib in ["ole32", "uuid", "avrt"] { println!("cargo:rustc-link-lib={lib}"); }
        if !target.contains("msvc") { println!("cargo:rustc-link-lib=stdc++"); }
    } else {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=pthread");
    }
    println!("cargo:rerun-if-changed=../CMakeLists.txt");
    println!("cargo:rerun-if-changed=../include");
    println!("cargo:rerun-if-changed=../src");
    #[cfg(feature = "ui")]
    slint_build::compile("ui/main.slint").expect("Compile OpenDAW interface");
}
