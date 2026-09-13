use std::path::Path;

fn main() {
    let default_z3_dir = r"D:\beam\z3-bin\z3-4.12.2-x64-win";
    let default_z3_header = r"D:\beam\z3-bin\z3-4.12.2-x64-win\include\z3.h";

    if Path::new(default_z3_header).exists() {
        println!("cargo:rustc-env=Z3_DIR={}", default_z3_dir);
        println!("cargo:rustc-env=Z3_SYS_Z3_HEADER={}", default_z3_header);
    }
}
