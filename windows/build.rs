use std::{env, path::PathBuf};
fn main() {
    let directory = env::var_os("FAH_NATIVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("native/build")
        });
    println!("cargo:rerun-if-env-changed=FAH_NATIVE_DIR");
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=dylib=fah_backend");
}
