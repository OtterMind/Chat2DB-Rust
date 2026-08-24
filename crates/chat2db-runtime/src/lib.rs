//! Shared runtime resource discovery for desktop, Web, and headless hosts.

use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use chat2db_core::{RuntimeConfig, load_fixed_community_classpath};
use chat2db_java_bridge::{BridgeError, EngineCommand, EngineConfig};
use thiserror::Error;

pub const DATA_DIR_ENV: &str = "CHAT2DB_DATA_DIR";
pub const DRIVER_PACK_DIR_ENV: &str = "CHAT2DB_DRIVER_PACK_DIR";
pub const COMMUNITY_CLASSPATH_DIR_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";
pub const JAVA_BIN_ENV: &str = "CHAT2DB_JAVA_BIN";
pub const JAVA_ENGINE_JAR_ENV: &str = "CHAT2DB_JAVA_ENGINE_JAR";
pub const VAULT_MASTER_KEY_ENV: &str = "CHAT2DB_VAULT_MASTER_KEY";

const BUNDLED_JAVA_BIN: &str = "Java binary";
const BUNDLED_JAVA_ENGINE_JAR: &str = "compatibility-engine JAR";
const BUNDLED_COMMUNITY_CLASSPATH: &str = "Community classpath";
const BUNDLED_DRIVER_PACKS: &str = "driver packs";

/// Explicit host inputs layered over process environment and packaged resources.
#[derive(Debug, Default)]
pub struct RuntimeOptions<'a> {
    pub data_dir: Option<PathBuf>,
    pub executable: Option<&'a Path>,
    pub resource_dir: Option<&'a Path>,
}

/// Runtime resource lookup or validation failure.
#[derive(Debug, Error)]
pub enum RuntimeConfigError {
    #[error("{JAVA_ENGINE_JAR_ENV} is required and must point to the compatibility-engine JAR")]
    MissingJavaEngineJar,
    #[error("{0} must not be empty when configured")]
    EmptyEnvironmentVariable(&'static str),
    #[error("{JAVA_ENGINE_JAR_ENV} does not point to a regular file: {}", .0.display())]
    InvalidJavaEngineJar(PathBuf),
    #[error("unable to inspect {JAVA_ENGINE_JAR_ENV} at {}: {source}", path.display())]
    JavaEngineJarMetadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("bundled {resource} is missing or is not a {expected}: {}", path.display())]
    InvalidBundledResource {
        resource: &'static str,
        expected: &'static str,
        path: PathBuf,
    },
    #[error("{VAULT_MASTER_KEY_ENV} must contain valid UTF-8")]
    InvalidVaultMasterKeyEncoding,
    #[error("fixed Community classpath failed validation: {0}")]
    CommunityClasspath(#[source] Box<BridgeError>),
}

#[derive(Debug, Default)]
struct RuntimeResourceOverrides {
    java_bin: Option<OsString>,
    java_engine_jar: Option<OsString>,
    community_classpath_dir: Option<OsString>,
    driver_pack_dir: Option<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeResourcePaths {
    java_bin: OsString,
    java_engine_jar: PathBuf,
    community_classpath_dir: Option<PathBuf>,
    driver_pack_dir: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
struct BundledRuntimeResources {
    java_bin: PathBuf,
    java_engine_jar: PathBuf,
    community_classpath_dir: PathBuf,
    driver_pack_dir: PathBuf,
}

impl BundledRuntimeResources {
    fn from_resource_dir(resource_dir: &Path) -> Option<Self> {
        resource_dir
            .is_absolute()
            .then(|| Self::from_resource_root(&resource_dir.join("chat2db")))
    }

    fn from_resource_root(resource_root: &Path) -> Self {
        Self {
            java_bin: resource_root
                .join("java")
                .join("bin")
                .join(if cfg!(windows) { "java.exe" } else { "java" }),
            java_engine_jar: resource_root
                .join("engine")
                .join("chat2db-compat-runtime.jar"),
            community_classpath_dir: resource_root.join("community-classpath"),
            driver_pack_dir: resource_root.join("driver-packs"),
        }
    }

    fn from_executable(executable: &Path) -> Option<Self> {
        let executable_dir = executable.parent()?;

        if executable_dir.file_name() == Some(std::ffi::OsStr::new("MacOS")) {
            let contents_dir = executable_dir.parent()?;
            if contents_dir.file_name() == Some(std::ffi::OsStr::new("Contents"))
                && contents_dir
                    .parent()
                    .is_some_and(|path| path.extension() == Some(std::ffi::OsStr::new("app")))
            {
                return Some(Self::from_resource_root(
                    &contents_dir.join("Resources").join("chat2db"),
                ));
            }
        }

        [
            executable_dir.parent().map(Path::to_path_buf),
            Some(executable_dir.join("resources").join("chat2db")),
            executable_dir
                .parent()
                .map(|path| path.join("resources").join("chat2db")),
            executable_dir
                .parent()
                .map(|path| path.join("lib").join("chat2db")),
        ]
        .into_iter()
        .flatten()
        .find(|root| {
            root.join("engine")
                .join("chat2db-compat-runtime.jar")
                .is_file()
        })
        .map(|root| Self::from_resource_root(&root))
    }
}

/// Builds one lazy-Java runtime configuration shared by every delivery mode.
///
/// Explicit `data_dir` wins over `CHAT2DB_DATA_DIR`. Resource environment
/// overrides win over packaged resource discovery.
///
/// # Errors
///
/// Returns a validation error when configured or packaged runtime resources
/// are missing, unsafe, or incompatible.
pub fn runtime_config_from_environment(
    options: RuntimeOptions<'_>,
) -> Result<RuntimeConfig, RuntimeConfigError> {
    let overrides = RuntimeResourceOverrides {
        java_engine_jar: optional_nonempty_os_env(JAVA_ENGINE_JAR_ENV)?,
        java_bin: optional_nonempty_os_env(JAVA_BIN_ENV)?,
        community_classpath_dir: optional_nonempty_os_env(COMMUNITY_CLASSPATH_DIR_ENV)?,
        driver_pack_dir: optional_nonempty_os_env(DRIVER_PACK_DIR_ENV)?,
    };
    let resources =
        resolve_runtime_resource_paths(options.executable, options.resource_dir, overrides)?;
    let mut engine = EngineConfig::new(EngineCommand::java_jar(
        resources.java_bin,
        resources.java_engine_jar,
    ));
    if let Some(community_classpath_dir) = resources.community_classpath_dir {
        let classpath = load_fixed_community_classpath(community_classpath_dir)
            .map_err(|error| RuntimeConfigError::CommunityClasspath(Box::new(error)))?;
        engine = engine.with_community_classpath(classpath);
    }
    let mut config = RuntimeConfig::new(engine);

    let data_dir = match options.data_dir {
        Some(data_dir) => Some(data_dir),
        None => optional_nonempty_os_env(DATA_DIR_ENV)?.map(PathBuf::from),
    };
    if let Some(data_dir) = data_dir {
        config = config.with_data_dir(data_dir);
    }
    if let Some(driver_pack_dir) = resources.driver_pack_dir {
        config = config.with_driver_pack_dir(driver_pack_dir);
    }
    match env::var(VAULT_MASTER_KEY_ENV) {
        Ok(master_key) => config = config.with_vault_master_key_base64(master_key),
        Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_)) => {
            return Err(RuntimeConfigError::InvalidVaultMasterKeyEncoding);
        }
    }
    Ok(config)
}

fn resolve_runtime_resource_paths(
    executable: Option<&Path>,
    resource_dir: Option<&Path>,
    overrides: RuntimeResourceOverrides,
) -> Result<RuntimeResourcePaths, RuntimeConfigError> {
    let bundled = resource_dir
        .and_then(BundledRuntimeResources::from_resource_dir)
        .or_else(|| executable.and_then(BundledRuntimeResources::from_executable));

    let java_bin = match overrides.java_bin {
        Some(java_bin) => java_bin,
        None => match bundled.as_ref() {
            Some(resources) => {
                validate_bundled_file(BUNDLED_JAVA_BIN, &resources.java_bin)?;
                resources.java_bin.clone().into_os_string()
            }
            None => OsString::from(if cfg!(windows) { "java.exe" } else { "java" }),
        },
    };
    let java_engine_jar = match overrides.java_engine_jar {
        Some(java_engine_jar) => {
            let path = PathBuf::from(java_engine_jar);
            validate_java_engine_jar(&path)?;
            path
        }
        None => match bundled.as_ref() {
            Some(resources) => {
                validate_bundled_file(BUNDLED_JAVA_ENGINE_JAR, &resources.java_engine_jar)?;
                resources.java_engine_jar.clone()
            }
            None => return Err(RuntimeConfigError::MissingJavaEngineJar),
        },
    };
    let community_classpath_dir = resolve_directory_override(
        overrides.community_classpath_dir,
        bundled
            .as_ref()
            .map(|resources| &resources.community_classpath_dir),
        BUNDLED_COMMUNITY_CLASSPATH,
    )?;
    let driver_pack_dir = resolve_directory_override(
        overrides.driver_pack_dir,
        bundled.as_ref().map(|resources| &resources.driver_pack_dir),
        BUNDLED_DRIVER_PACKS,
    )?;

    Ok(RuntimeResourcePaths {
        java_bin,
        java_engine_jar,
        community_classpath_dir,
        driver_pack_dir,
    })
}

fn resolve_directory_override(
    override_path: Option<OsString>,
    bundled_path: Option<&PathBuf>,
    resource: &'static str,
) -> Result<Option<PathBuf>, RuntimeConfigError> {
    match override_path {
        Some(directory) => Ok(Some(PathBuf::from(directory))),
        None => match bundled_path {
            Some(directory) => {
                validate_bundled_directory(resource, directory)?;
                Ok(Some((*directory).clone()))
            }
            None => Ok(None),
        },
    }
}

fn optional_nonempty_os_env(name: &'static str) -> Result<Option<OsString>, RuntimeConfigError> {
    validate_optional_os_env(name, env::var_os(name))
}

fn validate_optional_os_env(
    name: &'static str,
    value: Option<OsString>,
) -> Result<Option<OsString>, RuntimeConfigError> {
    match value {
        Some(value) if value.is_empty() => Err(RuntimeConfigError::EmptyEnvironmentVariable(name)),
        value => Ok(value),
    }
}

fn validate_java_engine_jar(path: &Path) -> Result<(), RuntimeConfigError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(RuntimeConfigError::InvalidJavaEngineJar(path.to_path_buf())),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(RuntimeConfigError::InvalidJavaEngineJar(path.to_path_buf()))
        }
        Err(source) => Err(RuntimeConfigError::JavaEngineJarMetadata {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_bundled_file(resource: &'static str, path: &Path) -> Result<(), RuntimeConfigError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) | Err(_) => Err(RuntimeConfigError::InvalidBundledResource {
            resource,
            expected: "regular file",
            path: path.to_path_buf(),
        }),
    }
}

fn validate_bundled_directory(
    resource: &'static str,
    path: &Path,
) -> Result<(), RuntimeConfigError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) | Err(_) => Err(RuntimeConfigError::InvalidBundledResource {
            resource,
            expected: "directory",
            path: path.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs::{self, File},
        path::PathBuf,
    };

    use super::{
        BUNDLED_COMMUNITY_CLASSPATH, BUNDLED_DRIVER_PACKS, BUNDLED_JAVA_BIN,
        BUNDLED_JAVA_ENGINE_JAR, BundledRuntimeResources, RuntimeConfigError,
        RuntimeResourceOverrides, resolve_runtime_resource_paths, validate_java_engine_jar,
        validate_optional_os_env,
    };

    fn complete_app_bundle() -> (tempfile::TempDir, PathBuf, BundledRuntimeResources) {
        let directory = tempfile::tempdir().expect("temporary app bundle");
        let executable = directory
            .path()
            .join("Chat2DB.app")
            .join("Contents")
            .join("MacOS")
            .join("chat2db");
        fs::create_dir_all(executable.parent().expect("bundle executable parent"))
            .expect("bundle executable directory");
        File::create(&executable).expect("bundle executable");

        let resources = BundledRuntimeResources::from_executable(&executable)
            .expect("synthetic executable must be recognized as an app bundle");
        fs::create_dir_all(resources.java_bin.parent().expect("Java binary parent"))
            .expect("bundled Java directory");
        File::create(&resources.java_bin).expect("bundled Java binary");
        fs::create_dir_all(
            resources
                .java_engine_jar
                .parent()
                .expect("engine JAR parent"),
        )
        .expect("bundled engine directory");
        File::create(&resources.java_engine_jar).expect("bundled engine JAR");
        fs::create_dir_all(&resources.community_classpath_dir)
            .expect("bundled Community classpath");
        fs::create_dir_all(&resources.driver_pack_dir).expect("bundled driver packs");

        (directory, executable, resources)
    }

    #[test]
    fn macos_app_bundle_supplies_all_default_runtime_resources() {
        let (_directory, executable, bundled) = complete_app_bundle();
        let resolved = resolve_runtime_resource_paths(
            Some(&executable),
            None,
            RuntimeResourceOverrides::default(),
        )
        .expect("complete app bundle must resolve");

        assert_eq!(resolved.java_bin, bundled.java_bin.into_os_string());
        assert_eq!(resolved.java_engine_jar, bundled.java_engine_jar);
        assert_eq!(
            resolved.community_classpath_dir,
            Some(bundled.community_classpath_dir)
        );
        assert_eq!(resolved.driver_pack_dir, Some(bundled.driver_pack_dir));
    }

    #[test]
    fn resource_directory_supplies_non_macos_runtime_resources() {
        let directory = tempfile::tempdir().expect("temporary resource directory");
        let resource_dir = directory.path().join("resources");
        let bundled = BundledRuntimeResources::from_resource_dir(&resource_dir)
            .expect("absolute resource directory must resolve");
        fs::create_dir_all(bundled.java_bin.parent().expect("Java binary parent"))
            .expect("bundled Java directory");
        File::create(&bundled.java_bin).expect("bundled Java binary");
        fs::create_dir_all(bundled.java_engine_jar.parent().expect("engine JAR parent"))
            .expect("bundled engine directory");
        File::create(&bundled.java_engine_jar).expect("bundled engine JAR");
        fs::create_dir_all(&bundled.community_classpath_dir).expect("bundled Community classpath");
        fs::create_dir_all(&bundled.driver_pack_dir).expect("bundled driver packs");

        let resolved = resolve_runtime_resource_paths(
            None,
            Some(&resource_dir),
            RuntimeResourceOverrides::default(),
        )
        .expect("resource directory must resolve");
        assert_eq!(resolved.java_bin, bundled.java_bin.into_os_string());
        assert_eq!(resolved.java_engine_jar, bundled.java_engine_jar);
    }

    #[test]
    fn embedded_cli_discovers_its_sibling_runtime_resources() {
        let directory = tempfile::tempdir().expect("temporary resource root");
        let resource_root = directory.path().join("chat2db");
        let executable = resource_root.join("bin").join("chat2db");
        let bundled = BundledRuntimeResources::from_resource_root(&resource_root);
        fs::create_dir_all(executable.parent().expect("CLI parent")).expect("CLI directory");
        File::create(&executable).expect("CLI executable");
        fs::create_dir_all(bundled.java_bin.parent().expect("Java binary parent"))
            .expect("bundled Java directory");
        File::create(&bundled.java_bin).expect("bundled Java binary");
        fs::create_dir_all(bundled.java_engine_jar.parent().expect("engine JAR parent"))
            .expect("bundled engine directory");
        File::create(&bundled.java_engine_jar).expect("bundled engine JAR");
        fs::create_dir_all(&bundled.community_classpath_dir).expect("Community classpath");
        fs::create_dir_all(&bundled.driver_pack_dir).expect("driver packs");

        let resolved = resolve_runtime_resource_paths(
            Some(&executable),
            None,
            RuntimeResourceOverrides::default(),
        )
        .expect("embedded CLI layout must resolve");
        assert_eq!(resolved.java_engine_jar, bundled.java_engine_jar);
        assert_eq!(resolved.driver_pack_dir, Some(bundled.driver_pack_dir));
    }

    #[test]
    fn app_bundle_reports_each_missing_runtime_resource() {
        for missing_resource in [
            BUNDLED_JAVA_BIN,
            BUNDLED_JAVA_ENGINE_JAR,
            BUNDLED_COMMUNITY_CLASSPATH,
            BUNDLED_DRIVER_PACKS,
        ] {
            let (_directory, executable, bundled) = complete_app_bundle();
            let (missing_path, is_directory) = match missing_resource {
                BUNDLED_JAVA_BIN => (bundled.java_bin, false),
                BUNDLED_JAVA_ENGINE_JAR => (bundled.java_engine_jar, false),
                BUNDLED_COMMUNITY_CLASSPATH => (bundled.community_classpath_dir, true),
                BUNDLED_DRIVER_PACKS => (bundled.driver_pack_dir, true),
                _ => unreachable!("all bundled resources are covered"),
            };
            if is_directory {
                fs::remove_dir_all(&missing_path).expect("remove bundled directory");
            } else {
                fs::remove_file(&missing_path).expect("remove bundled file");
            }

            let error = resolve_runtime_resource_paths(
                Some(&executable),
                None,
                RuntimeResourceOverrides::default(),
            )
            .expect_err("missing bundled resource must fail closed");
            assert!(matches!(
                error,
                RuntimeConfigError::InvalidBundledResource { resource, path, .. }
                    if resource == missing_resource && path == missing_path
            ));
        }
    }

    #[test]
    fn development_executable_still_requires_java_engine_environment() {
        let directory = tempfile::tempdir().expect("temporary development layout");
        let executable = directory
            .path()
            .join("target")
            .join("debug")
            .join("chat2db");

        assert!(matches!(
            resolve_runtime_resource_paths(
                Some(&executable),
                None,
                RuntimeResourceOverrides::default(),
            ),
            Err(RuntimeConfigError::MissingJavaEngineJar)
        ));
    }

    #[test]
    fn optional_path_environment_rejects_explicit_empty_values() {
        assert!(matches!(
            validate_optional_os_env("CHAT2DB_DRIVER_PACK_DIR", Some(OsString::new())),
            Err(RuntimeConfigError::EmptyEnvironmentVariable(
                "CHAT2DB_DRIVER_PACK_DIR"
            ))
        ));
        assert_eq!(
            validate_optional_os_env("CHAT2DB_DRIVER_PACK_DIR", None)
                .expect("missing optional variable must be accepted"),
            None
        );
    }

    #[test]
    fn environment_paths_override_missing_bundle_resources() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let java_bin = directory.path().join("java");
        let engine_jar = directory.path().join("engine.jar");
        File::create(&java_bin).expect("Java binary");
        File::create(&engine_jar).expect("engine JAR");

        let resolved = resolve_runtime_resource_paths(
            None,
            None,
            RuntimeResourceOverrides {
                java_bin: Some(java_bin.clone().into_os_string()),
                java_engine_jar: Some(engine_jar.clone().into_os_string()),
                community_classpath_dir: Some(OsString::from("community")),
                driver_pack_dir: Some(OsString::from("drivers")),
            },
        )
        .expect("explicit overrides must resolve");
        assert_eq!(resolved.java_bin, java_bin.into_os_string());
        assert_eq!(resolved.java_engine_jar, engine_jar);
    }

    #[test]
    fn missing_engine_jar_is_rejected() {
        assert!(matches!(
            resolve_runtime_resource_paths(None, None, RuntimeResourceOverrides::default(),),
            Err(RuntimeConfigError::MissingJavaEngineJar)
        ));
    }

    #[test]
    fn engine_jar_must_be_a_regular_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(matches!(
            validate_java_engine_jar(directory.path()),
            Err(RuntimeConfigError::InvalidJavaEngineJar(_))
        ));
    }
}
