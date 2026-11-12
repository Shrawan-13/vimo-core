//! The build script.
//!
//! The Rust libraries link against the Oboe shim that CMake builds, so the
//! linker needs to find it. The shim lands in `app/src/main/jniLibs/<abi>`,
//! which is where the Gradle build also collects the packaged libraries.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=VIMO_NATIVE_LIB_DIR");
    println!("cargo:rerun-if-env-changed=TARGET");

    let target = env::var("TARGET").unwrap_or_default();
    let abi = match target.as_str() {
        "aarch64-linux-android" => "arm64-v8a",
        "armv7-linux-androideabi" => "armeabi-v7a",
        "i686-linux-android" => "x86",
        "x86_64-linux-android" => "x86_64",
        _ => "",
    };
    if abi.is_empty() {
        // Not an Android target: the desktop build has no Oboe shim, so the
        // platform layer is not compiled in.
        return;
    }

    let dir = env::var("VIMO_NATIVE_LIB_DIR").unwrap_or_else(|_| {
        // The default is the layout of a local checkout of this repository.
        "../../app/src/main/jniLibs".to_string()
    });
    println!("cargo:rustc-link-search=native={dir}/{abi}");
    println!("cargo:rustc-link-lib=dylib=vimo_oboe");
}
