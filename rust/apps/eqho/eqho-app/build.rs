//! Embeds Windows resources (PerMonitorV2 manifest, `assets/eqho.ico`, and VERSIONINFO)
//! into `Eqho.exe` as a pure-Rust COFF `.rsrc` object via `damt-build` (no `rc.exe`/`mt.exe`).
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../../../assets/eqho.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| ".".into()));
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_else(|| ".".into()));

    let manifest = embed_manifest::new_manifest("Eqho")
        .dpi_awareness(embed_manifest::manifest::DpiAwareness::PerMonitorV2);

    let ico_path = manifest_dir.join("../../../../assets/eqho.ico");
    if !ico_path.is_file() {
        panic!("eqho.ico not found at {}", ico_path.display());
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let version_info = damt_build::VersionInfo {
        company_name: "damt.xyz".to_owned(),
        product_name: "Eqho".to_owned(),
        file_version: version.clone(),
        product_version: version,
        legal_copyright: "Copyright (C) 2026 Daniel Mitev (Daniel Mevit)".to_owned(),
    };

    let target = std::env::var("TARGET").unwrap_or_default();
    let resources = damt_build::ResourceBuilder::new()
        .manifest(&manifest.to_string())
        .icon(&ico_path)
        .version_info(version_info)
        .write_coff(&out_dir, "eqho-resources", &target)
        .unwrap_or_else(|err| panic!("failed to build Eqho Windows resources: {err}"));

    for arg in &resources.link_args {
        println!("cargo:rustc-link-arg-bins={arg}");
    }
}
