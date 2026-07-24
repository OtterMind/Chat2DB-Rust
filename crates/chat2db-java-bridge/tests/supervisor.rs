use std::{sync::Arc, time::Duration};

use chat2db_engine_protocol::{MAX_FRAME_BYTES, MIN_FRAME_BYTES, wire::ProtocolVersion};
use chat2db_java_bridge::{
    BridgeError, DeliveryOutcome, EngineClient, EngineCommand, EngineConfig, EngineState,
    EngineSupervisor,
};

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
    let supervisor =
        EngineSupervisor::spawn(fast_config(fixture_command(&["--hang-after-shutdown-ack"])))
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
    let supervisor = EngineSupervisor::spawn(fast_config(fixture_command(&["--ignore-shutdown"])))
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

async fn wait_for_stderr(supervisor: &EngineSupervisor, needle: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if supervisor
                .stderr_snapshot()
                .await
                .to_string_lossy()
                .contains(needle)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture must emit the expected stderr marker");
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
