.PHONY: verify rust rust-process-tests java ipc-integration jdbc-h2-integration \
	product-h2-integration frontend-deps frontend desktop generate-contracts \
	check-contracts

JAVA_ENGINE_JAR := $(CURDIR)/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar
H2_DRIVER_JAR := $(CURDIR)/java/compat-runtime/target/test-drivers/h2-2.3.232.jar

verify: rust rust-process-tests java ipc-integration jdbc-h2-integration \
	product-h2-integration frontend desktop

rust:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo test --workspace --locked

rust-process-tests:
	cargo test -p chat2db-java-bridge --features test-fixture --test supervisor --locked

java:
	cd java && ./mvnw -B clean verify

ipc-integration: java
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" CHAT2DB_H2_DRIVER_JAR="$(H2_DRIVER_JAR)" cargo test -p chat2db-java-bridge --features java-integration --test java_engine --locked

jdbc-h2-integration: java
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" CHAT2DB_H2_DRIVER_JAR="$(H2_DRIVER_JAR)" cargo test -p chat2db-java-bridge --features java-integration --test java_jdbc_h2 --locked

product-h2-integration: java
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" CHAT2DB_H2_DRIVER_JAR="$(H2_DRIVER_JAR)" cargo test -p chat2db-core --features java-integration --test java_h2_product --locked

frontend-deps:
	cd apps/frontend && npm ci

generate-contracts: frontend-deps
	./scripts/generate-contracts.sh

check-contracts: frontend-deps
	./scripts/check-contracts.sh

frontend: frontend-deps check-contracts
	cd apps/frontend && npm run typecheck && npm test && npm run build

desktop: frontend
	cargo test -p chat2db-desktop --locked
	cargo check -p chat2db-desktop --all-targets --features custom-protocol --locked
