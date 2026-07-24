use std::{ffi::OsString, path::Path};

use tokio::process::Command;

/// Executable and arguments used to launch one compatibility engine.
#[derive(Clone, Debug)]
pub struct EngineCommand {
    program: OsString,
    arguments: Vec<OsString>,
}

impl EngineCommand {
    /// Creates a process command without invoking a shell.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
        }
    }

    /// Creates the normal `java -jar <engine>` process command.
    pub fn java_jar(java: impl Into<OsString>, jar: impl AsRef<Path>) -> Self {
        Self::new(java)
            .arg("-jar")
            .arg(jar.as_ref().as_os_str().to_owned())
    }

    /// Appends one literal process argument.
    #[must_use]
    pub fn arg(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    pub(crate) fn build(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.arguments);
        command
    }
}
