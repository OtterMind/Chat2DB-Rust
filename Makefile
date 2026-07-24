.PHONY: verify rust java frontend

verify: rust java frontend

rust:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo test --workspace --locked

java:
	cd java && ./mvnw -B clean verify

frontend:
	cd apps/frontend && npm ci && npm run build
