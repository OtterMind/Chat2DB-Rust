//! Safe wrappers around the Windows primitives required by local IPC.

#[cfg(any(windows, test))]
use std::io;

/// Selects whether a named pipe is the first server instance for its name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipeInstanceKind {
    /// Refuses creation if another server has already claimed the pipe name.
    First,
    /// Adds another server instance after the first one has been secured.
    Additional,
}

#[cfg(any(windows, test))]
#[derive(Debug, Eq, PartialEq)]
struct PipeConfiguration {
    first_instance: bool,
    max_instances: usize,
}

#[cfg(any(windows, test))]
fn pipe_configuration(
    instance: PipeInstanceKind,
    max_instances: u8,
) -> io::Result<PipeConfiguration> {
    if !(1..=254).contains(&max_instances) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "named pipe max_instances must be between 1 and 254",
        ));
    }
    Ok(PipeConfiguration {
        first_instance: matches!(instance, PipeInstanceKind::First),
        max_instances: usize::from(max_instances),
    })
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp;

#[cfg(windows)]
pub use imp::{
    create_new_owner_only_file, create_owner_only_named_pipe, is_named_pipe_busy,
    open_or_create_owner_only_file, open_owner_only_file, replace_file,
    secure_owner_only_directory, verify_named_pipe_server, verify_owner_only_directory,
};

#[cfg(test)]
mod tests {
    use std::io;

    use super::{PipeConfiguration, PipeInstanceKind, pipe_configuration};

    #[test]
    fn configures_first_and_additional_named_pipe_instances() {
        assert_eq!(
            pipe_configuration(PipeInstanceKind::First, 1).expect("minimum is valid"),
            PipeConfiguration {
                first_instance: true,
                max_instances: 1,
            }
        );
        assert_eq!(
            pipe_configuration(PipeInstanceKind::Additional, 254).expect("maximum is valid"),
            PipeConfiguration {
                first_instance: false,
                max_instances: 254,
            }
        );
    }

    #[test]
    fn rejects_reserved_named_pipe_instance_limits() {
        for value in [0, u8::MAX] {
            let error = pipe_configuration(PipeInstanceKind::First, value)
                .expect_err("limit must be rejected");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
    }
}
