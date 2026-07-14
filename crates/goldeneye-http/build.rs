use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let dist = manifest.join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut files = Vec::new();
    collect_files(&dist, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "UI distribution is missing: {}",
        dist.display()
    );

    let mut generated = String::from("pub static ASSETS: &[EmbeddedAsset] = &[\n");
    for file in files {
        let relative = file
            .strip_prefix(&dist)
            .expect("asset below dist")
            .to_string_lossy()
            .replace('\\', "/");
        let absolute = file.canonicalize().expect("canonical UI asset");
        writeln!(
            generated,
            "    EmbeddedAsset {{ path: {relative:?}, bytes: include_bytes!({absolute:?}) }},",
            absolute = absolute.to_string_lossy(),
        )
        .expect("write generated asset entry");
    }
    generated.push_str("];\n");

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("build output")).join("assets.rs");
    fs::write(output, generated).expect("write generated asset table");
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("UI asset entry").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}
