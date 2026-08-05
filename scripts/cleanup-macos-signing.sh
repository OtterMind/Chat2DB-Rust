#!/usr/bin/env bash
set -euo pipefail

keychain_path="${CHAT2DB_SIGNING_KEYCHAIN:-}"
if [[ -z "${keychain_path}" ]]; then
  exit 0
fi

if [[ -z "${RUNNER_TEMP:-}" || "${RUNNER_TEMP}" != /* || ! -d "${RUNNER_TEMP}" || -L "${RUNNER_TEMP}" ]]; then
  echo "refusing to clean a signing keychain without a safe runner temp directory" >&2
  exit 1
fi
case "${keychain_path}" in
  "${RUNNER_TEMP}"/chat2db-signing-*.keychain-db) ;;
  *)
    echo "refusing to clean unexpected signing keychain: ${keychain_path}" >&2
    exit 1
    ;;
esac

remaining_keychains=()
while IFS= read -r existing_keychain; do
  existing_keychain="${existing_keychain//\"/}"
  existing_keychain="${existing_keychain#"${existing_keychain%%[![:space:]]*}"}"
  existing_keychain="${existing_keychain%"${existing_keychain##*[![:space:]]}"}"
  if [[ -n "${existing_keychain}" && "${existing_keychain}" != "${keychain_path}" ]]; then
    remaining_keychains+=("${existing_keychain}")
  fi
done < <(security list-keychains -d user)
if [[ "${#remaining_keychains[@]}" -gt 0 ]]; then
  security list-keychains -d user -s "${remaining_keychains[@]}"
fi
security delete-keychain "${keychain_path}" >/dev/null 2>&1 || true
rm -f -- "${keychain_path}"
echo "Removed ephemeral macOS signing keychain"
