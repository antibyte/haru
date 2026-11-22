// Compiles only the bundled C++ sources in this directory (no network, no external commands).
// Upstream additionally runs bindgen (needs libclang); the 20-line FFI surface is hand-written
// in src/lib.rs instead.
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=src/wrapper.h");
    println!("cargo::rerun-if-changed=src/wrapper.cpp");
    println!("cargo::rerun-if-changed=signalsmith-stretch/signalsmith-stretch.h");

    cc::Build::new()
        .file(Path::new("src").join("wrapper.cpp"))
        .include(Path::new("signalsmith-stretch"))
        .include(Path::new("."))
        .cpp(true)
        .std("c++14")
        .compile("signalsmith-stretch");
}
