fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rerun-if-env-changed=FAH_NATIVE_DIR");
        let path = std::env::var("FAH_NATIVE_DIR")
            .expect("Set FAH_NATIVE_DIR to the Android backend directory");
        println!("cargo:rustc-link-search=native={path}");
        println!("cargo:rustc-link-lib=dylib=fah_backend");
    }
}
