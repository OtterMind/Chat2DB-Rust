use std::{env, ffi::OsString, io, net::SocketAddr, path::PathBuf, process::ExitCode};

use chat2db_core::{RuntimeConfig, RuntimeHost};
use chat2db_local::LocalServer;
use chat2db_runtime::RuntimeOptions;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:4200";
const DEFAULT_FRONTEND_DIR: &str = "apps/frontend/dist";

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
    let executable = env::current_exe().ok();
    chat2db_runtime::runtime_config_from_environment(RuntimeOptions {
        data_dir: None,
        executable: executable.as_deref(),
        resource_dir: None,
    })
    .map_err(Into::into)
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
