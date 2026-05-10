use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    // Read contract source files and compile them
    let contracts_dir = Path::new("src");

    // Compile counter.rs
    compile_contract(&contracts_dir.join("counter.rs"), &out_dir);

    println!("cargo::rerun-if-changed=src/counter.rs");
}

fn compile_contract(source_path: &Path, out_dir: &str) {
    let source = fs::read_to_string(source_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", source_path.display(), e));

    let artifact = oxyde_codegen::compile_contract_source(&source)
        .unwrap_or_else(|e| panic!("failed to compile {}: {}", source_path.display(), e));

    artifact.write_to(out_dir)
        .unwrap_or_else(|e| panic!("failed to write artifacts for {}: {}", source_path.display(), e));
}
