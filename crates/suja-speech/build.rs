fn main() {
    println!("cargo:rustc-link-arg-examples=-Wl,-rpath,$ORIGIN");
}
