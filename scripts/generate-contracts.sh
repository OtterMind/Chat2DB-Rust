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

mkdir -p "$(dirname -- "$OPENAPI_PATH")" "$(dirname -- "$TYPESCRIPT_PATH")"
temporary_openapi="$(mktemp "${TMPDIR:-/tmp}/chat2db-openapi.XXXXXX")"
temporary_typescript="$(mktemp "${TMPDIR:-/tmp}/chat2db-contract.XXXXXX")"
trap 'rm -f "$temporary_openapi" "$temporary_typescript"' EXIT

cd "$ROOT_DIR"
cargo run --quiet --locked -p chat2db-web --bin generate-openapi >"$temporary_openapi"
"$OPENAPI_TYPESCRIPT" "$temporary_openapi" -o "$temporary_typescript"

mv "$temporary_openapi" "$OPENAPI_PATH"
mv "$temporary_typescript" "$TYPESCRIPT_PATH"
