#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OPENAPI_PATH="$ROOT_DIR/contracts/openapi/chat2db-v1.json"
TYPESCRIPT_PATH="$ROOT_DIR/apps/frontend/src/generated/contract.ts"
OPENAPI_TYPESCRIPT="$ROOT_DIR/apps/frontend/node_modules/.bin/openapi-typescript"

if [[ ! -x "$OPENAPI_TYPESCRIPT" ]]; then
  echo "openapi-typescript is not installed; run npm ci in apps/frontend" >&2
  exit 1
fi

temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/chat2db-contract-check.XXXXXX")"
trap 'rm -rf "$temporary_dir"' EXIT

cd "$ROOT_DIR"
cargo run --quiet --locked -p chat2db-web --bin generate-openapi \
  >"$temporary_dir/chat2db-v1.json"
"$OPENAPI_TYPESCRIPT" "$temporary_dir/chat2db-v1.json" \
  -o "$temporary_dir/contract.ts"

status=0
if ! cmp -s "$OPENAPI_PATH" "$temporary_dir/chat2db-v1.json"; then
  echo "contracts/openapi/chat2db-v1.json is stale; run make generate-contracts" >&2
  diff -u "$OPENAPI_PATH" "$temporary_dir/chat2db-v1.json" || true
  status=1
fi
if ! cmp -s "$TYPESCRIPT_PATH" "$temporary_dir/contract.ts"; then
  echo "apps/frontend/src/generated/contract.ts is stale; run make generate-contracts" >&2
  diff -u "$TYPESCRIPT_PATH" "$temporary_dir/contract.ts" || true
  status=1
fi

exit "$status"
