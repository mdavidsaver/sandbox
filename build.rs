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
        .allowlist_type("cap_user_header_t")
        .allowlist_type("cap_user_data_t")
        .allowlist_type("ifreq")
        .allowlist_function("capset")
        .allowlist_function("capget")
        .allowlist_function("ioctl")
        .allowlist_var("_LINUX_CAPABILITY_U32S_3")
        .allowlist_var("_LINUX_CAPABILITY_VERSION_3")
        .allowlist_var("CAP_SYS_ADMIN")
        .allowlist_var("CAP_SETUID")
        .allowlist_var("CAP_SETGID")
        .allowlist_var("SIOCGIFFLAGS")
        .allowlist_var("SIOCSIFFLAGS")
        .allowlist_var("SIOCGIFADDR")
        .allowlist_var("SIOCSIFADDR")
        .allowlist_var("SIOCGIFINDEX")
        .allowlist_var("SIOCSIFMTU")
        .allowlist_var("SIOCBRADDBR")
        .allowlist_var("SIOCBRADDIF")
        .allowlist_var("REAL_TUNSETIFF")
        .allowlist_var("IFF_UP")
        .allowlist_var("IFF_TAP")
        .allowlist_var("IFF_NO_PI")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("external.rs"))
        .expect("Couldn't write bindings!");
}
