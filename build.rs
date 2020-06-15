// https://rust-lang.github.io/rust-bindgen
extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=external.h");

    let bindings = bindgen::Builder::default()
        .header("external.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .derive_default(true)
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("external.rs"))
        .expect("Couldn't write bindings!");
}
