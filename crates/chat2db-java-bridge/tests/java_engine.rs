use std::{path::PathBuf, time::Duration};

use chat2db_engine_protocol::wire::ProtocolVersion;
use chat2db_java_bridge::{
    BridgeError, EngineCommand, EngineConfig, EngineState, EngineSupervisor,
};

fn java_config() -> EngineConfig {
    let jar = PathBuf::from(
        std::env::var_os("CHAT2DB_JAVA_ENGINE_JAR")
            .expect("CHAT2DB_JAVA_ENGINE_JAR must point to the packaged engine"),
    );
    assert!(
        jar.is_file(),
        "Java engine JAR does not exist: {}",
        jar.display()
    );
    EngineConfig::new(EngineCommand::java_jar("java", jar)).with_timeouts(
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(3),
    )
}

#[tokio::test]
async fn packaged_java_engine_handshakes_pings_and_shuts_down() {
    let supervisor = EngineSupervisor::spawn(java_config())
        .await
        .expect("packaged Java engine must handshake");
    let pong = supervisor
        .client()
        .ping(2026)
        .await
        .expect("Java ping must succeed");
    assert_eq!(pong.nonce, 2026);
    assert!(
        supervisor
            .shutdown()
            .await
            .expect("shutdown must succeed")
            .success
    );
}

#[tokio::test]
async fn packaged_java_engine_reports_protocol_incompatibility() {
    let result =
        EngineSupervisor::spawn(java_config().with_supported_versions(vec![ProtocolVersion {
            major: 99,
            minor: 0,
        }]))
        .await;
    let Err(error) = result else {
        panic!("incompatible host must be rejected");
    };
    assert!(matches!(
        error,
        BridgeError::Remote(remote) if remote.code == "protocol.unsupported_version" && remote.fatal
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn externally_killed_java_engine_is_reported_as_crashed() {
    let supervisor = EngineSupervisor::spawn(java_config())
        .await
        .expect("packaged Java engine must handshake");
    let process_id = supervisor
        .process_id()
        .expect("Unix Java process must expose a process id");
    let kill_status = std::process::Command::new("kill")
        .args(["-KILL", &process_id.to_string()])
        .status()
        .expect("kill command must run");
    assert!(kill_status.success());

    let mut states = supervisor.subscribe_state();
    let state = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let current = states.borrow().clone();
            if current.is_terminal() {
                return current;
            }
            states
                .changed()
                .await
                .expect("state sender must remain open");
        }
    })
    .await
    .expect("killed engine must reach a terminal state");
    assert!(matches!(state, EngineState::Crashed { .. }));
}
