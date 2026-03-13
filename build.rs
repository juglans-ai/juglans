use std::{env, fs, io::Write, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("stdlib_generated.rs");
    let mut f = fs::File::create(&dest).unwrap();

    writeln!(f, "pub fn get(name: &str) -> Option<&'static str> {{").unwrap();
    writeln!(f, "    match name {{").unwrap();

    // Embed files from stdlib/ and std/ directories
    for dir in &["stdlib", "std"] {
        let dir_path = Path::new(dir);
        if dir_path.exists() {
            for entry in fs::read_dir(dir_path).unwrap().flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "jg").unwrap_or(false) {
                    let stem = path.file_stem().unwrap().to_str().unwrap();
                    let abs = fs::canonicalize(&path).unwrap();
                    // Use forward slashes for include_str! paths — Windows backslashes
                    // are interpreted as escape sequences in Rust string literals.
                    let abs_str = abs.to_str().unwrap().replace('\\', "/");
                    // Strip \\?\ UNC prefix that canonicalize adds on Windows
                    let abs_str = abs_str.strip_prefix("//?/").unwrap_or(&abs_str);
                    writeln!(
                        f,
                        "        \"{}\" => Some(include_str!(\"{}\")),",
                        stem, abs_str
                    )
                    .unwrap();
                }
            }
        }
    }

    writeln!(f, "        _ => None,").unwrap();
    writeln!(f, "    }}").unwrap();
    writeln!(f, "}}").unwrap();

    println!("cargo:rerun-if-changed=stdlib");
    println!("cargo:rerun-if-changed=std");
}
