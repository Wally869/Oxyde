use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let contracts_dir = Path::new("src");

    // Read all contract sources into a map keyed by file stem.
    let mut sources: HashMap<String, String> = HashMap::new();
    for entry in fs::read_dir(contracts_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && path.file_name().and_then(|n| n.to_str()) != Some("lib.rs")
        {
            let source = fs::read_to_string(&path).unwrap();
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            sources.insert(stem, source);
        }
    }

    // Compile each contract with inheritance resolution.
    for (name, source) in &sources {
        let artifact = oxyde_codegen::compile_contract_with_deps(source, &sources)
            .unwrap_or_else(|e| panic!("failed to compile {name}: {e}"));
        artifact
            .write_to(&out_dir)
            .unwrap_or_else(|e| panic!("failed to write artifacts for {name}: {e}"));
    }

    println!("cargo::rerun-if-changed=src/");
}
