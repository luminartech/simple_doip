fn main() {
    // On Windows MSVC, the linker won't pull `main` from static libraries
    // unless explicitly told to. libfuzzer-sys compiles LLVM's FuzzerMain.cpp
    // (which defines `main`) into `fuzzer.lib`, but since nothing in the Rust
    // code references `main`, MSVC's linker drops it — causing LNK1561.
    //
    // Force the linker to include the `main` symbol from the fuzzer static lib.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/INCLUDE:main");
    }
}
