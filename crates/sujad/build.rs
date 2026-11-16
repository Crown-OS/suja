fn main() {
    println!("cargo:rerun-if-env-changed=SUJA_RUNTIME_LIB_DIR");
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    if let Ok(dir) = std::env::var("SUJA_RUNTIME_LIB_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
}
