use std::{env, path::PathBuf};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Tauri embeds its default manifest only in application binaries. Link the
        // same manifest into unit-test executables too, which need Common Controls v6.
        // See https://github.com/tauri-apps/tauri/issues/13419.
        let manifest = PathBuf::from(
            env::var_os("CARGO_MANIFEST_DIR")
                .expect("Cargo sets CARGO_MANIFEST_DIR for build scripts"),
        )
        .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("failed to run tauri-build");
    } else {
        tauri_build::build()
    }
}
