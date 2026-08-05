#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS signing configuration requires Darwin" >&2
  exit 1
fi

for variable in \
  MAC_CERTS \
  MAC_CERTS_PASSWORD \
  APPLE_ID \
  APPLE_APP_SPECIFIC_PASSWORD \
  APPLE_TEAM_ID \
  RUNNER_TEMP \
  GITHUB_ENV; do
  if [[ -z "${!variable:-}" ]]; then
    echo "required signing environment variable is missing: ${variable}" >&2
    exit 1
  fi
done

if [[ "${RUNNER_TEMP}" != /* || ! -d "${RUNNER_TEMP}" || -L "${RUNNER_TEMP}" ]]; then
  echo "refusing to use unsafe runner temp directory: ${RUNNER_TEMP}" >&2
  exit 1
fi
if [[ ! -f "${GITHUB_ENV}" || -L "${GITHUB_ENV}" ]]; then
  echo "refusing to use unsafe GitHub environment file: ${GITHUB_ENV}" >&2
  exit 1
fi

run_id="${GITHUB_RUN_ID:-manual}"
run_attempt="${GITHUB_RUN_ATTEMPT:-1}"
keychain_path="${RUNNER_TEMP}/chat2db-signing-${run_id}-${run_attempt}.keychain-db"
certificate_path="$(mktemp "${RUNNER_TEMP}/chat2db-signing.XXXXXX")"
notary_profile="chat2db-notary-${run_id}-${run_attempt}"
keychain_created=false
configured=false

cleanup() {
  rm -f -- "${certificate_path}"
  if [[ "${configured}" != true && "${keychain_created}" == true ]]; then
    security delete-keychain "${keychain_path}" >/dev/null 2>&1 || true
    rm -f -- "${keychain_path}"
  fi
}
trap cleanup EXIT

case "${keychain_path}" in
  "${RUNNER_TEMP}"/chat2db-signing-*.keychain-db) ;;
  *)
    echo "refusing to create unexpected signing keychain: ${keychain_path}" >&2
    exit 1
    ;;
esac
if [[ -e "${keychain_path}" || -L "${keychain_path}" ]]; then
  echo "signing keychain already exists: ${keychain_path}" >&2
  exit 1
fi

printf '%s' "${MAC_CERTS}" | tr -d '\r\n ' | /usr/bin/base64 -D > "${certificate_path}"
chmod 600 "${certificate_path}"
if [[ ! -s "${certificate_path}" ]]; then
  echo "decoded signing certificate is empty" >&2
  exit 1
fi

keychain_password="$(openssl rand -hex 32)"
security create-keychain -p "${keychain_password}" "${keychain_path}"
keychain_created=true
security set-keychain-settings -lut 21600 "${keychain_path}"
security unlock-keychain -p "${keychain_password}" "${keychain_path}"
security import "${certificate_path}" \
  -k "${keychain_path}" \
  -P "${MAC_CERTS_PASSWORD}" \
  -f pkcs12 \
  -T /usr/bin/codesign \
  -T /usr/bin/security
security set-key-partition-list \
  -S apple-tool:,apple: \
  -s \
  -k "${keychain_password}" \
  "${keychain_path}" >/dev/null

existing_keychains=()
while IFS= read -r existing_keychain; do
  existing_keychain="${existing_keychain//\"/}"
  existing_keychain="${existing_keychain#"${existing_keychain%%[![:space:]]*}"}"
  existing_keychain="${existing_keychain%"${existing_keychain##*[![:space:]]}"}"
  if [[ -n "${existing_keychain}" && "${existing_keychain}" != "${keychain_path}" ]]; then
    existing_keychains+=("${existing_keychain}")
  fi
done < <(security list-keychains -d user)
security list-keychains -d user -s "${keychain_path}" "${existing_keychains[@]}"

identity_output="$(security find-identity -v -p codesigning "${keychain_path}")"
signing_identity="$(awk '/\"Developer ID Application:/ { print $2; exit }' <<<"${identity_output}")"
if [[ -z "${signing_identity}" ]]; then
  echo "the imported archive does not contain a valid Developer ID Application identity" >&2
  printf '%s\n' "${identity_output}" >&2
  exit 1
fi

xcrun notarytool store-credentials "${notary_profile}" \
  --apple-id "${APPLE_ID}" \
  --password "${APPLE_APP_SPECIFIC_PASSWORD}" \
  --team-id "${APPLE_TEAM_ID}" \
  --keychain "${keychain_path}"

{
  printf 'APPLE_SIGNING_IDENTITY=%s\n' "${signing_identity}"
  printf 'APPLE_TEAM_ID=%s\n' "${APPLE_TEAM_ID}"
  printf 'CHAT2DB_SIGNING_KEYCHAIN=%s\n' "${keychain_path}"
  printf 'CHAT2DB_NOTARY_KEYCHAIN_PROFILE=%s\n' "${notary_profile}"
} >> "${GITHUB_ENV}"

configured=true
echo "Configured Developer ID signing identity ${signing_identity} in an ephemeral keychain"
