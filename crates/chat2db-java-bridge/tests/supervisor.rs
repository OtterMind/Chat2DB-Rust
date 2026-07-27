use std::{sync::Arc, time::Duration};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, MIN_FRAME_BYTES, wire::ProtocolVersion};
use chat2db_java_bridge::{
    BridgeError, BuildCommunityNamespaceSqlRequest, CancelDisposition, CommunityClasspath,
    CommunityNamespaceSqlOperation, DeliveryOutcome, DriverArtifact, DriverSpec, EngineClient,
    EngineCommand, EngineConfig, EngineState, EngineSupervisor, JdbcValue, QueryEvent,
    QueryOptions, QueryRequest, SESSION_JDBC_CAPABILITY, Session, SessionConfig, SessionState,
    TransactionOptions, UpdateRequest,
};

const COMMUNITY_COMMIT: &str = "f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7";

fn fixture_command(arguments: &[&str]) -> EngineCommand {
    arguments.iter().fold(
        EngineCommand::new(env!("CARGO_BIN_EXE_chat2db-engine-fixture")),
        |command, argument| command.arg(*argument),
    )
}

fn fast_config(command: EngineCommand) -> EngineConfig {
    EngineConfig::new(command).with_timeouts(
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
}

fn short_shutdown_config(command: EngineCommand) -> EngineConfig {
    EngineConfig::new(command).with_timeouts(
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_millis(200),
    )
}

#[tokio::test]
async fn handshakes_pings_and_reaps_after_shutdown() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[])))
        .await
        .expect("fixture must handshake");
    assert!(matches!(supervisor.state(), EngineState::Ready { .. }));

    let pong = supervisor
        .client()
        .ping(41)
        .await
        .expect("ping must succeed");
    assert_eq!(pong.nonce, 41);

    let exit = supervisor.shutdown().await.expect("shutdown must succeed");
    assert!(exit.success);
    assert!(matches!(supervisor.state(), EngineState::Stopped { .. }));
    assert!(
        supervisor
            .shutdown()
            .await
            .expect("shutdown must be idempotent")
            .success
    );
}

#[tokio::test]
async fn rejects_an_engine_without_a_common_protocol_version() {
    let result = EngineSupervisor::spawn(
        fast_config(fixture_command(&[])).with_supported_versions(vec![ProtocolVersion {
            major: 99,
            minor: 0,
        }]),
    )
    .await;
    let Err(error) = result else {
        panic!("incompatible protocol must fail startup");
    };

    assert!(matches!(
        error,
        BridgeError::Remote(remote) if remote.code == "protocol.unsupported_version" && remote.fatal
    ));
}

#[tokio::test]
async fn startup_timeout_kills_and_reaps_the_child() {
    let config = EngineConfig::new(fixture_command(&["--hang-before-handshake"])).with_timeouts(
        Duration::from_millis(50),
        Duration::from_secs(1),
        Duration::from_secs(1),
    );
    let result = EngineSupervisor::spawn(config).await;
    let Err(error) = result else {
        panic!("hung handshake must fail");
    };
    assert!(matches!(error, BridgeError::StartupTimeout));
}

#[tokio::test]
async fn configured_community_classpath_requires_capabilities_and_reaps_the_child() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let jar = directory.path().join("community-fixture.jar");
    std::fs::write(&jar, b"fixture").expect("fixture JAR must write");
    let snapshot_parent = directory.path().join("snapshots");
    std::fs::create_dir(&snapshot_parent).expect("snapshot parent must exist");
    let classpath = CommunityClasspath::from_paths(COMMUNITY_COMMIT, [jar])
        .expect("fixture Community classpath must validate");

    let result = EngineSupervisor::spawn(
        fast_config(fixture_command(&[]))
            .with_driver_snapshot_parent(&snapshot_parent)
            .with_community_classpath(classpath),
    )
    .await;
    let Err(error) = result else {
        panic!("an engine without Community capabilities must fail startup");
    };
    assert!(matches!(
        error,
        BridgeError::Remote(remote)
            if remote.code == "protocol.unsupported_capability" && remote.fatal
    ));
    assert!(
        std::fs::read_dir(&snapshot_parent)
            .expect("snapshot parent must remain readable")
            .next()
            .is_none(),
        "failed startup must reap the child and remove its generation snapshot"
    );
}

#[tokio::test]
async fn command_build_failure_removes_the_partial_generation_snapshot() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let jar = directory.path().join("community-fixture.jar");
    std::fs::write(&jar, b"fixture").expect("fixture JAR must write");
    let classpath = CommunityClasspath::from_paths(COMMUNITY_COMMIT, [jar.clone()])
        .expect("fixture Community classpath must validate");
    std::fs::remove_file(&jar).expect("fixture JAR must be removed before snapshotting");
    let snapshot_parent = directory.path().join("snapshots");
    std::fs::create_dir(&snapshot_parent).expect("snapshot parent must exist");

    let result = EngineSupervisor::spawn(
        fast_config(fixture_command(&[]))
            .with_driver_snapshot_parent(&snapshot_parent)
            .with_community_classpath(classpath),
    )
    .await;
    assert!(matches!(result, Err(BridgeError::CommunityArtifact { .. })));
    assert!(
        std::fs::read_dir(&snapshot_parent)
            .expect("snapshot parent must remain readable")
            .next()
            .is_none(),
        "command construction failure must remove its partial generation snapshot"
    );
}

#[tokio::test]
async fn process_spawn_failure_removes_the_generation_snapshot() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let snapshot_parent = directory.path().join("snapshots");
    std::fs::create_dir(&snapshot_parent).expect("snapshot parent must exist");
    let missing_executable = directory.path().join("missing-engine-executable");

    let result = EngineSupervisor::spawn(
        fast_config(EngineCommand::new(missing_executable))
            .with_driver_snapshot_parent(&snapshot_parent),
    )
    .await;
    assert!(matches!(result, Err(BridgeError::Spawn(_))));
    assert!(
        std::fs::read_dir(&snapshot_parent)
            .expect("snapshot parent must remain readable")
            .next()
            .is_none(),
        "spawn failure must remove its generation snapshot"
    );
}

#[tokio::test]
async fn correlates_concurrent_responses_that_arrive_in_reverse_order() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&["--reverse-pings=32"])))
        .await
        .expect("fixture must handshake");
    let client = supervisor.client();
    let mut tasks = Vec::new();
    for nonce in 0..32 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move { client.ping(nonce).await }));
    }
    for (nonce, task) in tasks.into_iter().enumerate() {
        let reply = task
            .await
            .expect("ping task must join")
            .expect("ping must succeed");
        assert_eq!(reply.nonce, u64::try_from(nonce).expect("nonce must fit"));
    }
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn unexpected_exit_fails_the_request_with_unknown_outcome() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--exit-on-ping",
        "--stderr-bytes=131072",
    ])))
    .await
    .expect("fixture must handshake even with a full stderr pipe");
    let error = supervisor
        .client()
        .ping(7)
        .await
        .expect_err("crashed engine must fail the ping");
    assert!(matches!(
        error,
        BridgeError::ProcessUnavailable {
            outcome: DeliveryOutcome::Unknown,
            ..
        } | BridgeError::CommandChannelClosed {
            outcome: DeliveryOutcome::Unknown,
        }
    ));

    let state = wait_for_terminal(&supervisor).await;
    let EngineState::Crashed { exit, .. } = state else {
        panic!("unexpected terminal state: {state:?}");
    };
    assert_eq!(exit.code, Some(42));
    assert!(exit.stderr.truncated);
    assert!(exit.stderr.total_bytes > 64 * 1024);
}

#[tokio::test]
async fn shutdown_timeout_force_kills_and_reaps_the_child() {
    let supervisor = EngineSupervisor::spawn(short_shutdown_config(fixture_command(&[
        "--hang-after-shutdown-ack",
    ])))
    .await
    .expect("fixture must handshake");
    let error = supervisor
        .shutdown()
        .await
        .expect_err("hung shutdown must be killed");
    assert!(matches!(error, BridgeError::ShutdownTimeout));
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Stopped { .. }
    ));
}

#[tokio::test]
async fn invalid_pong_fails_and_reaps_the_generation() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&["--wrong-pong"])))
        .await
        .expect("fixture must handshake");
    let error = supervisor
        .client()
        .ping(7)
        .await
        .expect_err("wrong nonce must violate the protocol");
    assert!(matches!(error, BridgeError::Protocol(_)));
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn mismatched_community_commit_fails_and_reaps_the_generation() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let jar = directory.path().join("community-fixture.jar");
    std::fs::write(&jar, b"fixture").expect("fixture JAR must write");
    let classpath = CommunityClasspath::from_paths(COMMUNITY_COMMIT, [jar])
        .expect("fixture Community classpath must validate");
    let supervisor = EngineSupervisor::spawn(
        fast_config(fixture_command(&["--community=wrong-commit"]))
            .with_community_classpath(classpath),
    )
    .await
    .expect("Community fixture must handshake");

    let error = supervisor
        .client()
        .community_client()
        .expect("Community client must bind")
        .list_plugins()
        .await
        .expect_err("a mismatched source commit must violate the protocol");
    assert!(matches!(error, BridgeError::Protocol(_)));
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn formatter_complexity_is_scoped_and_rejected_before_transport() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let jar = directory.path().join("community-fixture.jar");
    std::fs::write(&jar, b"fixture").expect("fixture JAR must write");
    let classpath = CommunityClasspath::from_paths(COMMUNITY_COMMIT, [jar])
        .expect("fixture Community classpath must validate");
    let supervisor = EngineSupervisor::spawn(
        fast_config(fixture_command(&["--community=wrong-commit"]))
            .with_community_classpath(classpath),
    )
    .await
    .expect("Community fixture must handshake");
    let community = supervisor
        .client()
        .community_client()
        .expect("Community client must bind");

    let exact = "a,".repeat(8_192);
    let exact_error = community
        .format_sql("H2", exact.clone())
        .await
        .expect_err("the exact complexity limit must reach the fixture");
    assert!(matches!(
        exact_error,
        BridgeError::Remote(remote) if remote.code == "community.not_configured"
    ));

    let over_limit = exact + "a";
    let formatter_error = community
        .format_sql("H2", over_limit.clone())
        .await
        .expect_err("formatter input above the complexity limit must fail locally");
    assert!(matches!(formatter_error, BridgeError::InvalidRequest(_)));
    assert!(formatter_error.to_string().contains("16384 units"));

    let parser_error = community
        .parse_sql("H2", over_limit)
        .await
        .expect_err("parser input must retain its independent byte-only contract");
    assert!(matches!(
        parser_error,
        BridgeError::Remote(remote) if remote.code == "community.not_configured"
    ));

    supervisor.shutdown().await.expect("fixture must shut down");
}

#[tokio::test]
async fn namespace_builder_dispatches_closed_operations_after_local_preflight() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let jar = directory.path().join("community-fixture.jar");
    std::fs::write(&jar, b"fixture").expect("fixture JAR must write");
    let classpath = CommunityClasspath::from_paths(COMMUNITY_COMMIT, [jar])
        .expect("fixture Community classpath must validate");
    let supervisor = EngineSupervisor::spawn(
        fast_config(fixture_command(&["--community=wrong-commit"]))
            .with_community_classpath(classpath),
    )
    .await
    .expect("Community fixture must negotiate the namespace capability");
    let community = supervisor
        .client()
        .community_client()
        .expect("Community client must bind");

    let remote = community
        .build_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: "H2".to_owned(),
            operation: CommunityNamespaceSqlOperation::DropSchema {
                schema_name: "APP".to_owned(),
            },
        })
        .await
        .expect_err("valid namespace request must reach the fixture");
    assert!(matches!(
        remote,
        BridgeError::Remote(remote) if remote.code == "community.not_configured"
    ));

    let local = community
        .build_namespace_sql(BuildCommunityNamespaceSqlRequest {
            database_type: "H2".to_owned(),
            operation: CommunityNamespaceSqlOperation::DropSchema {
                schema_name: "APP; DROP SCHEMA APP".to_owned(),
            },
        })
        .await
        .expect_err("unsafe namespace request must fail before transport");
    assert!(matches!(local, BridgeError::InvalidRequest(_)));

    assert!(matches!(supervisor.state(), EngineState::Ready { .. }));
    supervisor.shutdown().await.expect("fixture must shut down");
}

#[tokio::test]
async fn delivered_community_timeout_fails_and_reaps_the_generation() {
    let directory = tempfile::tempdir().expect("fixture directory must exist");
    let jar = directory.path().join("community-fixture.jar");
    std::fs::write(&jar, b"fixture").expect("fixture JAR must write");
    let classpath = CommunityClasspath::from_paths(COMMUNITY_COMMIT, [jar])
        .expect("fixture Community classpath must validate");
    let config = EngineConfig::new(fixture_command(&["--community=hang-catalog"]))
        .with_timeouts(
            Duration::from_secs(2),
            Duration::from_millis(120),
            Duration::from_millis(200),
        )
        .with_community_classpath(classpath);
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("Community fixture must handshake");

    let error = supervisor
        .client()
        .community_client()
        .expect("Community client must bind")
        .list_plugins()
        .await
        .expect_err("a delivered hanging Community request must time out");
    assert!(
        unknown_delivery(&error),
        "Community timeout must have unknown delivery: {error}"
    );
    wait_for_stderr(
        &supervisor,
        "fixture received hanging Community catalog request",
    )
    .await;
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn invalid_shutdown_ack_fails_and_reaps_the_generation() {
    let supervisor =
        EngineSupervisor::spawn(fast_config(fixture_command(&["--wrong-shutdown-response"])))
            .await
            .expect("fixture must handshake");
    let error = supervisor
        .shutdown()
        .await
        .expect_err("wrong shutdown body must violate the protocol");
    assert!(matches!(error, BridgeError::UnexpectedResponse(_)));
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn partial_stdout_frame_survives_concurrent_commands() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&["--split-pong"])))
        .await
        .expect("fixture must handshake");
    let client = supervisor.client();
    let first_client = client.clone();
    let first = tokio::spawn(async move { first_client.ping(1).await });
    wait_for_stderr(&supervisor, "fixture wrote split frame header").await;
    let second_client = client.clone();
    let second = tokio::spawn(async move { second_client.ping(2).await });

    assert_eq!(
        first
            .await
            .expect("first ping task must join")
            .expect("first ping must survive the split frame")
            .nonce,
        1
    );
    assert_eq!(
        second
            .await
            .expect("second ping task must join")
            .expect("second ping must succeed")
            .nonce,
        2
    );
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn cancelled_request_releases_its_in_flight_slot() {
    let supervisor = EngineSupervisor::spawn(
        fast_config(fixture_command(&["--ignore-first-ping"])).with_max_in_flight(1),
    )
    .await
    .expect("fixture must handshake");
    let client = supervisor.client();
    let first_client = client.clone();
    let first = tokio::spawn(async move { first_client.ping(1).await });
    wait_for_stderr(&supervisor, "fixture ignored first ping").await;
    first.abort();
    assert!(
        first
            .await
            .expect_err("aborted ping must be cancelled")
            .is_cancelled()
    );

    let second = client
        .ping(2)
        .await
        .expect("cancelled request must release the only in-flight slot");
    assert_eq!(second.nonce, 2);
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn dropping_the_owner_terminates_and_reaps_with_live_clients() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[])))
        .await
        .expect("fixture must handshake");
    let client = supervisor.client();
    drop(supervisor);

    assert!(matches!(
        wait_for_client_terminal(&client).await,
        EngineState::Stopped { .. }
    ));
}

#[tokio::test]
async fn exit_immediately_after_handshake_cannot_leave_a_ready_state() {
    let result =
        EngineSupervisor::spawn(fast_config(fixture_command(&["--exit-after-handshake"]))).await;
    match result {
        Ok(supervisor) => {
            assert!(matches!(
                wait_for_terminal(&supervisor).await,
                EngineState::Crashed { .. } | EngineState::Failed { .. }
            ));
        }
        Err(error) => assert!(matches!(
            error,
            BridgeError::ProcessUnavailable { .. }
                | BridgeError::CommandChannelClosed { .. }
                | BridgeError::SupervisorTask(_)
        )),
    }
}

#[tokio::test]
async fn shutdown_without_an_ack_is_reaped_before_returning() {
    let supervisor = EngineSupervisor::spawn(short_shutdown_config(fixture_command(&[
        "--ignore-shutdown",
    ])))
    .await
    .expect("fixture must handshake");
    let error = supervisor
        .shutdown()
        .await
        .expect_err("missing shutdown ack must fail");
    assert!(matches!(
        error,
        BridgeError::RequestTimeout {
            outcome: DeliveryOutcome::Unknown,
            ..
        }
    ));
    assert!(supervisor.state().is_terminal());
}

#[tokio::test]
async fn cancelling_shutdown_still_terminates_and_reaps() {
    let supervisor = Arc::new(
        EngineSupervisor::spawn(fast_config(fixture_command(&["--ignore-shutdown"])))
            .await
            .expect("fixture must handshake"),
    );
    let shutdown_supervisor = Arc::clone(&supervisor);
    let shutdown = tokio::spawn(async move { shutdown_supervisor.shutdown().await });
    wait_for_state(&supervisor, |state| {
        matches!(state, EngineState::Stopping { .. })
    })
    .await;
    shutdown.abort();
    assert!(
        shutdown
            .await
            .expect_err("shutdown task must be cancelled")
            .is_cancelled()
    );
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Stopped { .. }
    ));
}

#[tokio::test]
async fn late_non_terminal_response_is_a_protocol_failure() {
    let config = EngineConfig::new(fixture_command(&["--late-nonterminal-pong"])).with_timeouts(
        Duration::from_secs(2),
        Duration::from_millis(50),
        Duration::from_millis(200),
    );
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("fixture must handshake");
    let error = supervisor
        .client()
        .ping(7)
        .await
        .expect_err("ping must time out before the late response");
    assert!(matches!(error, BridgeError::RequestTimeout { .. }));
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn lifecycle_response_rejects_a_non_zero_sequence() {
    let supervisor =
        EngineSupervisor::spawn(fast_config(fixture_command(&["--non-zero-sequence"])))
            .await
            .expect("fixture must handshake");
    let error = supervisor
        .client()
        .ping(7)
        .await
        .expect_err("non-zero lifecycle sequence must fail");
    assert!(matches!(
        error,
        BridgeError::ProcessUnavailable {
            outcome: DeliveryOutcome::Unknown,
            ..
        } | BridgeError::CommandChannelClosed {
            outcome: DeliveryOutcome::Unknown,
        }
    ));
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn configured_frame_limit_enforces_both_boundaries() {
    let minimum = u32::try_from(MIN_FRAME_BYTES).expect("minimum frame size must fit u32");
    let maximum = u32::try_from(MAX_FRAME_BYTES).expect("maximum frame size must fit u32");
    for invalid in [minimum - 1, maximum + 1] {
        let result = EngineSupervisor::spawn(
            fast_config(fixture_command(&[])).with_max_receive_frame_bytes(invalid),
        )
        .await;
        assert!(matches!(result, Err(BridgeError::InvalidConfig(_))));
    }

    for valid in [minimum, maximum] {
        let supervisor = EngineSupervisor::spawn(
            fast_config(fixture_command(&[])).with_max_receive_frame_bytes(valid),
        )
        .await
        .expect("boundary frame size must be accepted");
        let EngineState::Ready { identity, .. } = supervisor.state() else {
            panic!("successful handshake must enter ready state");
        };
        assert_eq!(
            identity.max_frame_bytes, maximum,
            "the host receive limit must not reduce the independent peer receive limit"
        );
        supervisor.shutdown().await.expect("shutdown must succeed");
    }
}

#[tokio::test]
async fn rejects_peer_frame_limit_below_protocol_minimum() {
    let peer_limit = MIN_FRAME_BYTES - 1;
    let argument = format!("--peer-max-frame-bytes={peer_limit}");
    let result = EngineSupervisor::spawn(fast_config(fixture_command(&[&argument]))).await;
    assert!(matches!(result, Err(BridgeError::InvalidHandshake(_))));
}

#[tokio::test]
async fn oversized_outbound_frame_is_rejected_before_delivery() {
    let argument = format!("--peer-max-frame-bytes={MIN_FRAME_BYTES}");
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=paused",
        &argument,
    ])))
    .await
    .expect("fixture must negotiate the minimum frame size");
    let session = open_fixture_session(&supervisor).await;
    let mut request = fixture_query(0);
    request.sql = "x".repeat(MIN_FRAME_BYTES * 2);
    let Err(error) = session.execute_query(request).await else {
        panic!("oversized query frame must be rejected locally");
    };
    assert!(matches!(error, BridgeError::InvalidRequest(_)));
    assert!(matches!(supervisor.state(), EngineState::Ready { .. }));
    supervisor
        .client()
        .ping(7)
        .await
        .expect("local frame rejection must keep the generation ready");
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn jdbc_capabilities_are_checked_when_each_api_is_called() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[])))
        .await
        .expect("lifecycle-only fixture must handshake");
    let driver = supervisor
        .client()
        .driver_client()
        .expect("ready engine must create a generation-bound driver client");
    let Err(error) = driver
        .open_session(fixture_session_config("fixture-driver"))
        .await
    else {
        panic!("session API must reject a missing negotiated capability");
    };
    assert!(matches!(
        error,
        BridgeError::MissingCapability(capability) if capability == SESSION_JDBC_CAPABILITY
    ));
    assert!(matches!(supervisor.state(), EngineState::Ready { .. }));
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn jdbc_unary_driver_session_transaction_and_update_round_trip() {
    let supervisor =
        EngineSupervisor::spawn(fast_config(fixture_command(&["--jdbc-stream=normal"])))
            .await
            .expect("JDBC fixture must handshake");
    let client = supervisor.client();
    let driver = client
        .driver_client()
        .expect("ready engine must create a driver client");

    let artifact_path = std::env::temp_dir().join(format!(
        "chat2db-rust-driver-fixture-{}.jar",
        std::process::id()
    ));
    std::fs::write(&artifact_path, b"fixture driver jar")
        .expect("fixture artifact must be written");
    let artifact = DriverArtifact::from_path(&artifact_path)
        .expect("driver artifact must canonicalize and hash");
    assert!(artifact.canonical_path().is_absolute());
    assert_ne!(artifact.sha256(), &[0; 32]);
    let loaded = driver
        .load_driver(DriverSpec {
            driver_class: "fixture.Driver".to_owned(),
            artifacts: vec![artifact],
        })
        .await
        .expect("driver load must succeed");
    std::fs::remove_file(&artifact_path).expect("fixture artifact must be removed");
    assert!(loaded.driver_id.starts_with("sha256:"));
    assert_eq!(loaded.driver_id.len(), "sha256:".len() + 64);
    assert_eq!(loaded.artifact_count, 1);

    let session = driver
        .open_session(fixture_session_config(&loaded.driver_id))
        .await
        .expect("session open must succeed");
    assert_eq!(session.state().await, SessionState::AutoCommit);
    let transaction = session
        .begin_transaction(TransactionOptions::default())
        .await
        .expect("transaction begin must succeed");
    assert_eq!(session.state().await, SessionState::TransactionActive);
    session
        .commit_transaction(&transaction)
        .await
        .expect("transaction commit must succeed");
    assert_eq!(session.state().await, SessionState::AutoCommit);
    let transaction = session
        .begin_transaction(TransactionOptions::default())
        .await
        .expect("second transaction begin must succeed");
    session
        .rollback_transaction(&transaction)
        .await
        .expect("transaction rollback must succeed");
    assert_eq!(session.state().await, SessionState::AutoCommit);

    let update = session
        .execute_update(UpdateRequest {
            sql: "update fixture set value = 1".to_owned(),
            parameters: Vec::new(),
            transaction_id: None,
        })
        .await
        .expect("update must succeed");
    assert_eq!(update.affected_rows, 3);
    session.close().await.expect("session close must succeed");
    assert_eq!(session.state().await, SessionState::Closed);
    driver
        .unload_driver(loaded.driver_id)
        .await
        .expect("driver unload must succeed");
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn query_stream_delivers_typed_events_in_order() {
    let supervisor =
        EngineSupervisor::spawn(fast_config(fixture_command(&["--jdbc-stream=normal"])))
            .await
            .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut stream = session
        .execute_query(fixture_query(1))
        .await
        .expect("query must start");

    let Some(QueryEvent::Started(started)) = stream.next_event().await.expect("start must decode")
    else {
        panic!("first query event must contain columns");
    };
    assert_eq!(started.columns.len(), 1);
    let Some(QueryEvent::Batch(batch)) = stream.next_event().await.expect("batch must decode")
    else {
        panic!("second query event must be a row batch");
    };
    assert_eq!(batch.start_row_offset, 0);
    assert!(matches!(
        batch.rows[0].values.as_slice(),
        [JdbcValue::Text(value)] if value == "fixture-row"
    ));
    let Some(QueryEvent::Completed(completed)) =
        stream.next_event().await.expect("completion must decode")
    else {
        panic!("third query event must be completion");
    };
    assert_eq!(completed.row_count, 1);
    assert!(!completed.truncated_by_max_rows);
    assert!(!completed.truncated_by_max_result_bytes);
    assert!(
        stream
            .next_event()
            .await
            .expect("stream close must decode")
            .is_none()
    );
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn credit_control_lane_remains_available_when_normal_in_flight_is_full() {
    let supervisor = EngineSupervisor::spawn(
        fast_config(fixture_command(&["--jdbc-stream=paused"])).with_max_in_flight(1),
    )
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("paused query must start");
    assert!(matches!(
        stream.next_event().await.expect("start must decode"),
        Some(QueryEvent::Started(_))
    ));

    let accepted = stream
        .grant_credits(1)
        .await
        .expect("credit must use the reserved control lane");
    assert_eq!(accepted, 1);
    assert!(matches!(
        stream.next_event().await.expect("batch must decode"),
        Some(QueryEvent::Batch(_))
    ));
    assert!(matches!(
        stream.next_event().await.expect("completion must decode"),
        Some(QueryEvent::Completed(_))
    ));
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn query_accepts_immediate_credit_before_started() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=await-control",
    ])))
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must register before its first event");

    let accepted = stream
        .grant_credits(1)
        .await
        .expect("credit must be accepted before query-started");
    assert_eq!(accepted, 1);
    wait_for_stderr(
        &supervisor,
        "fixture received credit grant before query started",
    )
    .await;
    assert!(matches!(
        stream.next_event().await.expect("start must decode"),
        Some(QueryEvent::Started(_))
    ));
    assert!(matches!(
        stream.next_event().await.expect("batch must decode"),
        Some(QueryEvent::Batch(_))
    ));
    assert!(matches!(
        stream.next_event().await.expect("completion must decode"),
        Some(QueryEvent::Completed(_))
    ));
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn query_accepts_immediate_cancel_before_started() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=await-control",
    ])))
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must register before its first event");

    assert_eq!(
        stream
            .cancel(None)
            .await
            .expect("cancel must not report the query as inactive"),
        CancelDisposition::Accepted
    );
    wait_for_stderr(&supervisor, "fixture received cancel before query started").await;
    assert!(matches!(
        stream.next_event().await.expect("start must decode"),
        Some(QueryEvent::Started(_))
    ));
    assert!(matches!(
        stream.next_event().await.expect("completion must decode"),
        Some(QueryEvent::Completed(_))
    ));
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn delivered_query_registration_timeout_is_unknown_and_fatal() {
    let config = EngineConfig::new(fixture_command(&["--jdbc-stream=await-control"]))
        .with_timeouts(
            Duration::from_secs(2),
            Duration::from_millis(500),
            Duration::from_millis(200),
        )
        .with_registration_ack_delay_for_test(Duration::from_millis(1_500));
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;

    let Err(error) = session.execute_query(fixture_query(0)).await else {
        panic!("query registration must time out after delivery");
    };
    assert!(
        unknown_delivery(&error),
        "delivered registration timeout must have unknown outcome: {error}"
    );
    wait_for_stderr(&supervisor, "fixture received await-control query").await;
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn aborting_delivered_query_before_registration_ack_is_fatal() {
    let config = EngineConfig::new(fixture_command(&["--jdbc-stream=await-control"]))
        .with_timeouts(
            Duration::from_secs(2),
            Duration::from_secs(3),
            Duration::from_millis(200),
        )
        .with_registration_ack_delay_for_test(Duration::from_secs(1));
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let query = tokio::spawn(async move { session.execute_query(fixture_query(0)).await });
    wait_for_stderr(&supervisor, "fixture received await-control query").await;

    query.abort();
    let Err(join_error) = query.await else {
        panic!("query task must be cancelled");
    };
    assert!(join_error.is_cancelled());
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn delivered_update_deadline_fails_the_generation() {
    let config = EngineConfig::new(fixture_command(&[
        "--jdbc-stream=normal",
        "--hang-on-update",
    ]))
    .with_timeouts(
        Duration::from_secs(2),
        Duration::from_millis(120),
        Duration::from_millis(200),
    );
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;

    let error = session
        .execute_update(fixture_update())
        .await
        .expect_err("a delivered hanging update must time out");
    assert!(unknown_delivery(&error), "unexpected update error: {error}");
    wait_for_stderr(&supervisor, "fixture received hanging update").await;
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn aborting_delivered_update_fails_the_generation() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=normal",
        "--hang-on-update",
    ])))
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let update = tokio::spawn(async move { session.execute_update(fixture_update()).await });
    wait_for_stderr(&supervisor, "fixture received hanging update").await;

    update.abort();
    assert!(
        update
            .await
            .expect_err("update task must be cancelled")
            .is_cancelled()
    );
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn delivered_credit_deadline_fails_the_generation() {
    let config = EngineConfig::new(fixture_command(&[
        "--jdbc-stream=await-control",
        "--hang-on-grant-credits",
    ]))
    .with_timeouts(
        Duration::from_secs(2),
        Duration::from_millis(120),
        Duration::from_millis(200),
    );
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must register before its first event");

    let error = stream
        .grant_credits(1)
        .await
        .expect_err("a delivered hanging credit grant must time out");
    assert!(unknown_delivery(&error), "unexpected credit error: {error}");
    wait_for_stderr(&supervisor, "fixture received hanging credit grant").await;
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn aborting_delivered_credit_fails_the_generation() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=await-control",
        "--hang-on-grant-credits",
    ])))
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must register before its first event");

    let mut grant = Box::pin(stream.grant_credits(1));
    tokio::select! {
        () = wait_for_stderr(&supervisor, "fixture received hanging credit grant") => {}
        result = &mut grant => panic!("credit grant returned before abort: {result:?}"),
    }
    drop(grant);
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn delivered_cancel_deadline_fails_the_generation() {
    let config = EngineConfig::new(fixture_command(&[
        "--jdbc-stream=await-control",
        "--hang-on-cancel",
    ]))
    .with_timeouts(
        Duration::from_secs(2),
        Duration::from_millis(120),
        Duration::from_millis(200),
    );
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must register before its first event");

    let error = stream
        .cancel(None)
        .await
        .expect_err("a delivered hanging cancel must time out");
    assert!(unknown_delivery(&error), "unexpected cancel error: {error}");
    wait_for_stderr(&supervisor, "fixture received hanging cancel").await;
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn aborting_delivered_cancel_fails_the_generation() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=await-control",
        "--hang-on-cancel",
    ])))
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must register before its first event");

    let mut cancel = Box::pin(stream.cancel(None));
    tokio::select! {
        () = wait_for_stderr(&supervisor, "fixture received hanging cancel") => {}
        result = &mut cancel => panic!("cancel returned before abort: {result:?}"),
    }
    drop(cancel);
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn control_lane_rejects_the_seventeenth_in_flight_request() {
    let config = EngineConfig::new(fixture_command(&[
        "--jdbc-stream=await-control",
        "--hang-on-cancel",
    ]))
    .with_timeouts(
        Duration::from_secs(2),
        Duration::from_secs(5),
        Duration::from_millis(200),
    );
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut streams = Vec::new();
    for _ in 0..17 {
        streams.push(
            session
                .execute_query(fixture_query(0))
                .await
                .expect("query must register before its first event"),
        );
    }
    let rejected_stream = streams.pop().expect("the seventeenth stream must exist");
    let cancellations = streams
        .into_iter()
        .map(|stream| tokio::spawn(async move { stream.cancel(None).await }))
        .collect::<Vec<_>>();
    wait_for_stderr_count(&supervisor, "fixture received hanging cancel", 16).await;

    let error = rejected_stream
        .cancel(None)
        .await
        .expect_err("the seventeenth control request must be rejected locally");
    assert!(matches!(
        error,
        BridgeError::ProcessUnavailable {
            outcome: DeliveryOutcome::NotSent,
            ..
        }
    ));
    assert!(matches!(supervisor.state(), EngineState::Ready { .. }));

    for cancellation in &cancellations {
        cancellation.abort();
    }
    for cancellation in cancellations {
        assert!(
            cancellation
                .await
                .expect_err("cancel task must be cancelled")
                .is_cancelled()
        );
    }
    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn dropped_query_sends_one_cancel_and_validates_until_terminal() {
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
        "--jdbc-stream=cancel-completes",
    ])))
    .await
    .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must start");
    assert!(matches!(
        stream.next_event().await.expect("start must decode"),
        Some(QueryEvent::Started(_))
    ));
    drop(stream);

    wait_for_stderr(&supervisor, "fixture query cancel count 1").await;
    supervisor
        .client()
        .ping(99)
        .await
        .expect("generation must remain usable after abandoned stream terminates");
    let stderr = supervisor.stderr_snapshot().await.to_string_lossy();
    assert_eq!(stderr.matches("fixture query cancel count").count(), 1);
    supervisor.shutdown().await.expect("shutdown must succeed");
}

#[tokio::test]
async fn abandoned_query_timeout_fails_and_reaps_the_generation() {
    let config = EngineConfig::new(fixture_command(&["--jdbc-stream=cancel-hangs"])).with_timeouts(
        Duration::from_secs(2),
        Duration::from_millis(120),
        Duration::from_millis(200),
    );
    let supervisor = EngineSupervisor::spawn(config)
        .await
        .expect("JDBC fixture must handshake");
    let session = open_fixture_session(&supervisor).await;
    let mut stream = session
        .execute_query(fixture_query(0))
        .await
        .expect("query must start");
    assert!(matches!(
        stream.next_event().await.expect("start must decode"),
        Some(QueryEvent::Started(_))
    ));
    drop(stream);

    assert!(matches!(
        wait_for_terminal(&supervisor).await,
        EngineState::Failed { .. }
    ));
}

#[tokio::test]
async fn malformed_query_streams_fail_and_reap_the_generation() {
    let cases = [
        "gap",
        "duplicate",
        "row-before-started",
        "multiple-terminal",
        "after-terminal",
        "wrong-trace",
        "started-terminal",
        "completed-nonterminal",
        "wrong-offset",
        "wrong-column-count",
    ];
    for behavior in cases {
        let argument = format!("--jdbc-stream={behavior}");
        let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[&argument])))
            .await
            .expect("JDBC fixture must handshake");
        let session = open_fixture_session(&supervisor).await;
        let _stream = session
            .execute_query(fixture_query(2))
            .await
            .expect("malformed fixture query must enter the process path");
        assert!(
            matches!(
                wait_for_terminal(&supervisor).await,
                EngineState::Failed { .. }
            ),
            "stream behavior {behavior} must fail the generation"
        );
    }
}

#[tokio::test]
async fn write_and_commit_crashes_are_unknown_and_never_replayed() {
    for operation in ["update", "commit"] {
        let journal = std::env::temp_dir().join(format!(
            "chat2db-rust-{operation}-journal-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&journal);
        let journal_argument = format!("--write-journal={}", journal.display());
        let exit_argument = format!("--exit-on-{operation}");
        let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&[
            "--jdbc-stream=normal",
            &exit_argument,
            &journal_argument,
        ])))
        .await
        .expect("write-crash fixture must handshake");
        let session = open_fixture_session(&supervisor).await;

        let error = if operation == "update" {
            session
                .execute_update(UpdateRequest {
                    sql: "update fixture set value = 2".to_owned(),
                    parameters: Vec::new(),
                    transaction_id: None,
                })
                .await
                .expect_err("crashed update must have an unknown outcome")
        } else {
            let transaction = session
                .begin_transaction(TransactionOptions::default())
                .await
                .expect("fixture transaction must begin");
            session
                .commit_transaction(&transaction)
                .await
                .expect_err("crashed commit must have an unknown outcome")
        };
        assert!(
            unknown_delivery(&error),
            "unexpected {operation} error: {error}"
        );
        assert!(matches!(
            wait_for_terminal(&supervisor).await,
            EngineState::Crashed { .. }
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            std::fs::read_to_string(&journal).expect("fixture journal must exist"),
            format!("{operation}\n")
        );
        std::fs::remove_file(&journal).expect("fixture journal must be removed");
    }
}

fn unknown_delivery(error: &BridgeError) -> bool {
    matches!(
        error,
        BridgeError::ProcessUnavailable {
            outcome: DeliveryOutcome::Unknown,
            ..
        } | BridgeError::CommandChannelClosed {
            outcome: DeliveryOutcome::Unknown,
        } | BridgeError::RequestTimeout {
            outcome: DeliveryOutcome::Unknown,
            ..
        }
    )
}

fn fixture_session_config(driver_id: &str) -> SessionConfig {
    SessionConfig {
        driver_id: driver_id.to_owned(),
        jdbc_url: "jdbc:fixture:test".to_owned(),
        properties: Vec::new(),
        read_only: false,
    }
}

async fn open_fixture_session(supervisor: &EngineSupervisor) -> Session {
    supervisor
        .client()
        .driver_client()
        .expect("ready engine must create a driver client")
        .open_session(fixture_session_config("fixture-driver"))
        .await
        .expect("fixture session must open")
}

fn fixture_query(initial_batch_credits: u32) -> QueryRequest {
    QueryRequest {
        sql: "select value from fixture".to_owned(),
        parameters: Vec::new(),
        transaction_id: None,
        options: QueryOptions {
            initial_batch_credits,
            ..QueryOptions::default()
        },
    }
}

fn fixture_update() -> UpdateRequest {
    UpdateRequest {
        sql: "update fixture set value = 1".to_owned(),
        parameters: Vec::new(),
        transaction_id: None,
    }
}

async fn wait_for_stderr(supervisor: &EngineSupervisor, needle: &str) {
    wait_for_stderr_count(supervisor, needle, 1).await;
}

async fn wait_for_stderr_count(supervisor: &EngineSupervisor, needle: &str, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let matches = supervisor
                .stderr_snapshot()
                .await
                .to_string_lossy()
                .matches(needle)
                .count();
            if matches >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture must emit the expected number of stderr markers");
}

async fn wait_for_state(
    supervisor: &EngineSupervisor,
    predicate: impl Fn(&EngineState) -> bool,
) -> EngineState {
    let mut state = supervisor.subscribe_state();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let current = state.borrow().clone();
            if predicate(&current) {
                return current;
            }
            state
                .changed()
                .await
                .expect("state sender must remain open");
        }
    })
    .await
    .expect("engine must reach the expected state")
}

async fn wait_for_client_terminal(client: &EngineClient) -> EngineState {
    let mut state = client.subscribe_state();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let current = state.borrow().clone();
            if current.is_terminal() {
                return current;
            }
            state
                .changed()
                .await
                .expect("state sender must remain open until terminal");
        }
    })
    .await
    .expect("engine must reach a terminal state")
}

async fn wait_for_terminal(supervisor: &EngineSupervisor) -> EngineState {
    let mut state = supervisor.subscribe_state();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let current = state.borrow().clone();
            if current.is_terminal() {
                return current;
            }
            state
                .changed()
                .await
                .expect("state sender must remain open");
        }
    })
    .await
    .expect("engine must reach a terminal state")
}
