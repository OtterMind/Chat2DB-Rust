use std::{env, ffi::OsString, io, net::SocketAddr, path::PathBuf, process::ExitCode};

use chat2db_core::{RuntimeConfig, RuntimeHost, load_fixed_community_classpath};
use chat2db_java_bridge::{EngineCommand, EngineConfig};
use chat2db_local::LocalServer;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:4200";
const DEFAULT_FRONTEND_DIR: &str = "apps/frontend/dist";
const COMMUNITY_CLASSPATH_DIR_ENV: &str = "CHAT2DB_COMMUNITY_CLASSPATH_DIR";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "Chat2DB Web runtime failed");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let address: SocketAddr = env::var("CHAT2DB_BIND")
        .unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned())
        .parse()?;
    let access_policy =
        chat2db_web::AccessPolicy::for_bind(address, env::var("CHAT2DB_ACCESS_TOKEN").ok())?;
    let runtime_config = runtime_config_from_env()?;
    let frontend_dir = optional_nonempty_os_env("CHAT2DB_FRONTEND_DIR")?
        .map_or_else(|| PathBuf::from(DEFAULT_FRONTEND_DIR), PathBuf::from);
    let listener = TcpListener::bind(address).await?;
    let mut host = RuntimeHost::open(runtime_config).await?;
    let application = host.application();
    let mut local_server = match LocalServer::start(application.clone()) {
        Ok(server) => server,
        Err(error) => {
            if let Err(shutdown_error) = host.shutdown().await {
                tracing::error!(%shutdown_error, "runtime cleanup failed after local attachment startup error");
            }
            return Err(Box::new(error));
        }
    };
    let shutdown_application = application.clone();
    info!(%address, frontend_dir = %frontend_dir.display(), "Chat2DB Web runtime listening");

    let serve_result = axum::serve(
        listener,
        chat2db_web::router_with_policy_and_assets(application, access_policy, frontend_dir)
            .into_make_service(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        shutdown_application.begin_shutdown().await;
    })
    .await;
    let local_shutdown_result = local_server.shutdown().await;
    let shutdown_result = host.shutdown().await;

    if let Err(serve_error) = serve_result {
        if let Err(local_error) = local_shutdown_result {
            tracing::error!(%local_error, "local attachment cleanup also failed after Web serve error");
        }
        if let Err(shutdown_error) = shutdown_result {
            tracing::error!(%shutdown_error, "runtime cleanup also failed after Web serve error");
        }
        return Err(Box::new(serve_error));
    }
    if let Err(local_error) = local_shutdown_result {
        if let Err(runtime_error) = shutdown_result {
            tracing::error!(%runtime_error, "runtime cleanup also failed after local attachment shutdown error");
        }
        return Err(Box::new(local_error));
    }
    shutdown_result?;
    Ok(())
}

fn runtime_config_from_env() -> Result<RuntimeConfig, Box<dyn std::error::Error>> {
    let engine_jar = PathBuf::from(required_nonempty_os_env("CHAT2DB_JAVA_ENGINE_JAR")?);
    let java =
        optional_nonempty_os_env("CHAT2DB_JAVA_BIN")?.unwrap_or_else(|| OsString::from("java"));
    let mut engine = EngineConfig::new(EngineCommand::java_jar(java, engine_jar));
    if let Some(community_classpath_dir) = optional_nonempty_os_env(COMMUNITY_CLASSPATH_DIR_ENV)? {
        engine = engine.with_community_classpath(load_fixed_community_classpath(PathBuf::from(
            community_classpath_dir,
        ))?);
    }
    let mut config = RuntimeConfig::new(engine);

    if let Some(data_dir) = optional_nonempty_os_env("CHAT2DB_DATA_DIR")? {
        config = config.with_data_dir(PathBuf::from(data_dir));
    }
    if let Some(driver_pack_dir) = optional_nonempty_os_env("CHAT2DB_DRIVER_PACK_DIR")? {
        config = config.with_driver_pack_dir(PathBuf::from(driver_pack_dir));
    }
    if let Some(master_key) = optional_unicode_env("CHAT2DB_VAULT_MASTER_KEY")? {
        config = config.with_vault_master_key_base64(master_key);
    }

    Ok(config)
}

fn required_nonempty_os_env(name: &'static str) -> Result<OsString, io::Error> {
    optional_nonempty_os_env(name)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} is required and must not be empty"),
        )
    })
}

fn optional_nonempty_os_env(name: &'static str) -> Result<Option<OsString>, io::Error> {
    match env::var_os(name) {
        None => Ok(None),
        Some(value) if value.is_empty() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must not be empty when configured"),
        )),
        Some(value) => Ok(Some(value)),
    }
}

fn optional_unicode_env(name: &'static str) -> Result<Option<String>, env::VarError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error @ env::VarError::NotUnicode(_)) => Err(error),
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate())
            .expect("SIGTERM handler must be installable for graceful shutdown");
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    tracing::error!(%error, "failed to install Ctrl-C handler");
                }
            }
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install Ctrl-C handler");
    }
}
