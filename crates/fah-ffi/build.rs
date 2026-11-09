fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target == "android" || target == "linux" {
        println!("cargo:rerun-if-env-changed=FAH_NATIVE_DIR");
        let path = std::env::var("FAH_NATIVE_DIR")
            .expect("Set FAH_NATIVE_DIR to the platform backend directory");
        println!("cargo:rustc-link-search=native={path}");
        println!("cargo:rustc-link-lib=dylib=fah_backend");
        if target == "linux" {
            println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        }
    }
}
