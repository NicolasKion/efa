//! Embeds the data pack when there is one (`EFA_PACK`, or assets/data/efa-data.pack), and names
//! the refine commit efa builds against, which the pack's engine data is tied to.

use std::{env, path::PathBuf};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(efa_pack)");
    println!("cargo::rerun-if-env-changed=EFA_PACK");
    println!("cargo::rerun-if-changed=Cargo.lock");

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let requested = env::var_os("EFA_PACK").filter(|p| !p.is_empty());
    let pack = if let Some(path) = &requested {
        let path = manifest.join(path);
        println!("cargo::rerun-if-changed={}", path.display());
        path
    } else {
        // The folder always exists (it holds a .gitignore), so watching it notices a pack
        // appearing without rerunning on every build.
        println!("cargo::rerun-if-changed=assets/data");
        manifest.join("assets/data/efa-data.pack")
    };
    if pack.is_file() {
        println!("cargo::rustc-cfg=efa_pack");
        println!("cargo::rustc-env=EFA_PACK_FILE={}", pack.display());
    } else if requested.is_some() {
        panic!("EFA_PACK is set but {} is not a file", pack.display());
    }

    let lock = std::fs::read_to_string(manifest.join("Cargo.lock")).unwrap_or_default();
    println!("cargo::rustc-env=EFA_REFINE_REV={}", refine_rev(&lock));
}

/// The commit of refine-core in Cargo.lock, or "unknown".
fn refine_rev(lock: &str) -> &str {
    lock.split("[[package]]")
        .find(|p| p.contains("name = \"refine-core\""))
        .and_then(|p| p.lines().find_map(|l| l.strip_prefix("source = ")))
        .and_then(|source| source.trim_matches('"').rsplit_once('#'))
        .map_or("unknown", |(_, rev)| rev)
}
