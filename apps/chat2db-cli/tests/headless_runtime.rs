use std::{fs::File, process::Command, time::Duration};

use chat2db_local::LocalClient;
use serde_json::Value;

const TEST_VAULT_MASTER_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

#[tokio::test]
async fn auto_mode_starts_a_no_gui_host_and_reaps_it_after_idle() {
    let directory = tempfile::tempdir().expect("temporary product data directory");
    let engine_jar = directory.path().join("compatibility-engine.jar");
    File::create(&engine_jar).expect("placeholder engine JAR");

    let output = Command::new(env!("CARGO_BIN_EXE_chat2db"))
        .arg("--data-dir")
        .arg(directory.path())
        .arg("status")
        .env("CHAT2DB_JAVA_ENGINE_JAR", &engine_jar)
        .env("CHAT2DB_VAULT_MASTER_KEY", TEST_VAULT_MASTER_KEY)
        .env("CHAT2DB_CLI_RUNTIME_IDLE_SECONDS", "1")
        .output()
        .expect("auto CLI command must execute");
    assert!(
        output.status.success(),
        "auto CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let health: Value = serde_json::from_slice(&output.stdout).expect("health JSON");
    assert_eq!(health["status"], "ready");
    let engine = health["components"]
        .as_array()
        .expect("health components")
        .iter()
        .find(|component| component["id"] == "database-engine")
        .expect("database engine health");
    assert!(
        engine["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("Java is not running"))
    );

    let client = LocalClient::new(directory.path());
    client
        .health()
        .await
        .expect("headless host remains attached");
    let endpoint_metadata = directory.path().join("local-attachment-v1.json");

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !endpoint_metadata.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("headless host must exit after its idle timeout");
}
