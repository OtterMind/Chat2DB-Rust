use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let repository_root = manifest_directory.join("../..");
    let proto_directory = repository_root.join("proto");
    let protocol = proto_directory.join("chat2db/compat/v1/compat.proto");

    println!("cargo:rerun-if-changed={}", protocol.display());

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.compile_protos(&[protocol], &[proto_directory])?;
    Ok(())
}
