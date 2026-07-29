.PHONY: verify rust rust-process-tests java ipc-integration jdbc-h2-integration \
	community-h2-classpath community-h2-reproducibility community-java-h2-integration \
	community-h2-integration \
	community-product-h2-integration product-h2-integration mysql-driver-pack \
	native-mysql-integration community-product-mysql-integration \
	frontend-deps frontend-source frontend desktop generate-contracts check-contracts \
	macos-runtime macos-package-java macos-package macos-package-verify

JAVA_ENGINE_JAR := $(CURDIR)/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar
H2_DRIVER_JAR := $(CURDIR)/java/compat-runtime/target/test-drivers/h2-2.3.232.jar
COMMUNITY_CLASSPATH_DIR := $(CURDIR)/target/community-h2-classpath
MYSQL_DRIVER_PACK_DIR := $(CURDIR)/target/mysql-driver-packs
MYSQL_TEST_HOST ?= 127.0.0.1
MYSQL_TEST_PORT ?= 3306
MYSQL_TEST_JDBC_PARAMETERS ?= sslMode=DISABLED&allowPublicKeyRetrieval=true&serverTimezone=UTC&zeroDateTimeBehavior=CONVERT_TO_NULL&tinyInt1isBit=false

verify: rust rust-process-tests java ipc-integration jdbc-h2-integration \
	community-java-h2-integration community-h2-integration \
	community-product-h2-integration product-h2-integration frontend desktop

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

community-h2-classpath:
	./scripts/build-community-h2-classpath.sh

community-h2-reproducibility:
	./scripts/verify-community-h2-reproducibility.sh

community-java-h2-integration: java community-h2-classpath
	cd java && \
	CHAT2DB_COMMUNITY_CLASSPATH_DIR="$(COMMUNITY_CLASSPATH_DIR)" \
	CHAT2DB_COMMUNITY_SOURCE_COMMIT="37a34be858f2566b6b7fcf6c3f64183c1f560853" \
	./mvnw -B -pl compat-runtime \
	-Dtest='CommunityPluginRegistryTest#realCommunityH2BuildsAndExecutesBoundedDml,CommunityPluginRegistryTest#realCommunityMysqlRejectsBackslashCrossColumnInjection,CommunityPluginRegistryTest#realCommunityMysqlNormalizesBooleanAliasesAndBits,CommunityPluginRegistryTest#realCommunityH2BuildsNamespaceSqlWithoutOpeningJdbc,CommunityPluginRegistryTest#realCommunityMysqlBuildsDatabaseNamespaceSql,CommunityPluginRegistryTest#realCommunityMysqlBuildsBoundedTablePreviewSqlWithoutOpeningJdbc,CommunityPluginRegistryTest#realCommunityNamespaceMapsUnsupportedAndRejectsOversizedInput,JdbcProtocolLoopTest#communityDmlDispatchDoesNotRequireAJdbcSession,JdbcProtocolLoopTest#communityNamespaceDispatchDoesNotRequireAJdbcSession' \
	test

community-h2-integration: java community-h2-classpath
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" CHAT2DB_H2_DRIVER_JAR="$(H2_DRIVER_JAR)" CHAT2DB_COMMUNITY_CLASSPATH_DIR="$(COMMUNITY_CLASSPATH_DIR)" cargo test -p chat2db-java-bridge --features java-integration --test java_community_h2 --locked

community-product-h2-integration: java community-h2-classpath
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" CHAT2DB_H2_DRIVER_JAR="$(H2_DRIVER_JAR)" CHAT2DB_COMMUNITY_CLASSPATH_DIR="$(COMMUNITY_CLASSPATH_DIR)" cargo test -p chat2db-core --features java-integration --test java_community_product --locked

product-h2-integration: java
	CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" CHAT2DB_H2_DRIVER_JAR="$(H2_DRIVER_JAR)" cargo test -p chat2db-core --features java-integration --test java_h2_product --locked

mysql-driver-pack:
	./scripts/prepare-mysql-driver-pack.sh "$(MYSQL_DRIVER_PACK_DIR)"

native-mysql-integration:
	@test -n "$(MYSQL_TEST_USER)" || (echo "MYSQL_TEST_USER is required" >&2; exit 1)
	@test -n "$(MYSQL_TEST_PASSWORD)" || (echo "MYSQL_TEST_PASSWORD is required" >&2; exit 1)
	@MYSQL_TEST_HOST="$(MYSQL_TEST_HOST)" \
	MYSQL_TEST_PORT="$(MYSQL_TEST_PORT)" \
	MYSQL_TEST_USER="$(MYSQL_TEST_USER)" \
	MYSQL_TEST_PASSWORD="$(MYSQL_TEST_PASSWORD)" \
	MYSQL_TEST_REQUIRED="1" \
	cargo test -p chat2db-core --test native_mysql_product --locked
	@MYSQL_TEST_HOST="$(MYSQL_TEST_HOST)" \
	MYSQL_TEST_PORT="$(MYSQL_TEST_PORT)" \
	MYSQL_TEST_USER="$(MYSQL_TEST_USER)" \
	MYSQL_TEST_PASSWORD="$(MYSQL_TEST_PASSWORD)" \
	cargo test -p chat2db-core --test native_mysql_console_docker --locked -- --ignored
	@MYSQL_TEST_HOST="$(MYSQL_TEST_HOST)" \
	MYSQL_TEST_PORT="$(MYSQL_TEST_PORT)" \
	MYSQL_TEST_USER="$(MYSQL_TEST_USER)" \
	MYSQL_TEST_PASSWORD="$(MYSQL_TEST_PASSWORD)" \
	cargo test -p chat2db-web --test native_mysql_editable_ddl_docker --locked -- --ignored

community-product-mysql-integration: java community-h2-classpath mysql-driver-pack
	@test -n "$(MYSQL_TEST_USER)" || (echo "MYSQL_TEST_USER is required" >&2; exit 1)
	@test -n "$(MYSQL_TEST_PASSWORD)" || (echo "MYSQL_TEST_PASSWORD is required" >&2; exit 1)
	@CHAT2DB_JAVA_ENGINE_JAR="$(JAVA_ENGINE_JAR)" \
	CHAT2DB_COMMUNITY_CLASSPATH_DIR="$(COMMUNITY_CLASSPATH_DIR)" \
	MYSQL_TEST_DRIVER_PACK_DIR="$(MYSQL_DRIVER_PACK_DIR)" \
	MYSQL_TEST_HOST="$(MYSQL_TEST_HOST)" \
	MYSQL_TEST_PORT="$(MYSQL_TEST_PORT)" \
	MYSQL_TEST_USER="$(MYSQL_TEST_USER)" \
	MYSQL_TEST_PASSWORD="$(MYSQL_TEST_PASSWORD)" \
	MYSQL_TEST_REQUIRED="1" \
	MYSQL_TEST_JDBC_PARAMETERS="$(MYSQL_TEST_JDBC_PARAMETERS)" \
	cargo test -p chat2db-core --features java-integration --test java_community_mysql_product --locked

frontend-deps:
	cd apps/frontend && npm ci

frontend-source: frontend-deps
	cd apps/frontend && npm run verify-upstream

generate-contracts: frontend-deps
	./scripts/generate-contracts.sh

check-contracts: frontend-deps
	./scripts/check-contracts.sh

frontend: frontend-source check-contracts
	cd apps/frontend && npm run typecheck && npm test && npm run build

desktop: frontend
	cargo test -p chat2db-desktop --locked
	cargo check -p chat2db-desktop --all-targets --features custom-protocol --locked

macos-runtime:
	./scripts/build-macos-runtime.sh

macos-package-java: community-h2-classpath
	$(MAKE) java

macos-package: macos-package-java mysql-driver-pack frontend macos-runtime
	./scripts/build-macos-package.sh

macos-package-verify:
	./scripts/verify-macos-package.sh
