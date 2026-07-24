.PHONY: verify rust rust-process-tests java ipc-integration frontend

JAVA_ENGINE_JAR := $(CURDIR)/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar

verify: rust rust-process-tests java ipc-integration frontend

rust:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo test --workspace --locked

rust-process-tests:
	cargo test -p chat2db-java-bridge --features test-fixture --test supervisor --locked

java:
	cd java && ./mvnw -B clean verify

ipc-integration: java
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" cargo test -p chat2db-java-bridge --features java-integration --test java_engine --locked

frontend:
	cd apps/frontend && npm ci && npm run build
