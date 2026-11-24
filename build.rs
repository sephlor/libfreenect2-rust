//! Links the CUDA libraries the `cuda` feature calls through hand-written FFI.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=CUDA_HOME");

    // Cargo exposes enabled features to build scripts as CARGO_FEATURE_<NAME>.
    if env::var_os("CARGO_FEATURE_CUDA").is_none() {
        return;
    }

    let cuda_home = env::var("CUDA_HOME").unwrap_or_else(|_| "/usr/local/cuda".into());
    println!("cargo:rustc-link-search=native={cuda_home}/lib64");
    println!("cargo:rustc-link-lib=dylib=nvjpeg");
}
