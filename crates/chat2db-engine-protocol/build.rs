use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let repository_root = manifest_directory.join("../..");
    let proto_directory = repository_root.join("proto");
    let compatibility_protocol = proto_directory.join("chat2db/compat/v1/compat.proto");
    let jdbc_protocol = proto_directory.join("chat2db/compat/v1/jdbc.proto");

    println!(
        "cargo:rerun-if-changed={}",
        compatibility_protocol.display()
    );
    println!("cargo:rerun-if-changed={}", jdbc_protocol.display());

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.compile_protos(&[compatibility_protocol, jdbc_protocol], &[proto_directory])?;
    Ok(())
}
