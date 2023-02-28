//! Compile and generate Rust code from proto files.
//!

use std::env;
use std::path::PathBuf;
use vergen::{Config, vergen};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .type_attribute(
            ".",
            "#[derive(::serde::Deserialize)] #[serde(rename_all = \"snake_case\")]")
        .file_descriptor_set_path(out_dir.join("grpc_descriptor.bin"))
        .compile(&["proto/echo.proto"], &["proto"])
        .unwrap();

    // Generate the default 'cargo:' instruction output
    vergen(Config::default()).expect("Unable to generate the cargo keys!");

    // Deprecated
    if let Some(project_version) = option_env!("PROJECT_VERSION") {
        println!("cargo:rustc-env=PROJECT_VERSION= - {}", project_version);
    } else {
        println!("cargo:rustc-env=PROJECT_VERSION=");
    }

    Ok(())
}
