// Point the linker at a prebuilt Z3 when Z3_DIR is set (the layout of the
// official z3-<ver>-x64-win release zip: Z3_DIR/bin holds libz3.lib + libz3.dll).
// Without Z3_DIR the system linker search path is used unchanged.
fn main() {
    println!("cargo:rerun-if-env-changed=Z3_DIR");
    // Fallback kept from the previous build.rs: the Z3 release vendored under D:\beam.
    let dir = std::env::var("Z3_DIR")
        .ok()
        .or_else(|| Some(r"D:\beam\z3-bin\z3-4.12.2-x64-win".to_string()));
    if let Some(dir) = dir {
        let dir = std::path::Path::new(&dir);
        for sub in ["bin", "lib"] {
            let p = dir.join(sub);
            if p.is_dir() {
                println!("cargo:rustc-link-search=native={}", p.display());
            }
        }
    }
}
