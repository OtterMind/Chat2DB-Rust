#!/usr/bin/env bash
set -euo pipefail

readonly driver_version="8.1.2.141"
readonly driver_filename="DmJdbcDriver18-${driver_version}.jar"
readonly driver_sha256="8b8d7b18aa4f048b68700ac9d53661767bc6fdd857bbf48e928a8293289b96dc"
readonly driver_bytes="1030636"
readonly driver_url="https://cdn.chat2db-ai.com/lib/${driver_filename}"
readonly pack_directory_name="03-dm"

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-${repository_root}/target/driver-packs}"
source_jar="${DM_JDBC_DRIVER_JAR:-}"
staging_directory=""
backup_directory=""
pack_directory=""

sha256_file() {
  local path="$1"
  local output
  local digest

  if command -v sha256sum >/dev/null 2>&1; then
    output="$(sha256sum -- "${path}")"
  elif command -v shasum >/dev/null 2>&1; then
    output="$(shasum -a 256 -- "${path}")"
  elif command -v openssl >/dev/null 2>&1; then
    output="$(openssl dgst -sha256 -r "${path}")"
  else
    echo "sha256sum, shasum, or openssl is required" >&2
    return 1
  fi

  digest="${output%% *}"
  digest="$(printf '%s' "${digest}" | tr '[:upper:]' '[:lower:]')"
  if [[ ! "${digest}" =~ ^[0-9a-f]{64}$ ]]; then
    echo "invalid SHA-256 output for ${path}" >&2
    return 1
  fi
  printf '%s' "${digest}"
}

cleanup() {
  if [[ -n "${staging_directory}" && -d "${staging_directory}" ]]; then
    rm -rf -- "${staging_directory}"
  fi
  if [[ -n "${backup_directory}" && -d "${backup_directory}" ]]; then
    if [[ -n "${pack_directory}" && ! -e "${pack_directory}" ]]; then
      mv -- "${backup_directory}" "${pack_directory}"
    else
      rm -rf -- "${backup_directory}"
    fi
  fi
}
trap cleanup EXIT

if [[ -e "${output_root}" && ( ! -d "${output_root}" || -L "${output_root}" ) ]]; then
  echo "driver-pack root must be a non-symbolic directory: ${output_root}" >&2
  exit 1
fi
mkdir -p "${output_root}"
output_root="$(cd "${output_root}" && pwd -P)"

staging_directory="$(mktemp -d "${output_root}/.${pack_directory_name}.staging.XXXXXX")"
staged_jar="${staging_directory}/${driver_filename}"

if [[ -n "${source_jar}" ]]; then
  if [[ ! -f "${source_jar}" || -L "${source_jar}" ]]; then
    echo "DM_JDBC_DRIVER_JAR must be a non-symbolic regular file" >&2
    exit 1
  fi
  cp -- "${source_jar}" "${staged_jar}"
else
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required when DM_JDBC_DRIVER_JAR is not set" >&2
    exit 1
  fi
  curl --fail --location --silent --show-error \
    --retry 3 --retry-all-errors \
    --output "${staged_jar}" \
    "${driver_url}"
fi

actual_bytes="$(LC_ALL=C wc -c < "${staged_jar}" | tr -d '[:space:]')"
if [[ "${actual_bytes}" != "${driver_bytes}" ]]; then
  echo "DM JDBC driver byte length mismatch: expected ${driver_bytes}, found ${actual_bytes}" >&2
  exit 1
fi

actual_sha256="$(sha256_file "${staged_jar}")"
if [[ "${actual_sha256}" != "${driver_sha256}" ]]; then
  echo "DM JDBC driver SHA-256 mismatch: expected ${driver_sha256}, found ${actual_sha256}" >&2
  exit 1
fi

printf '%s\n' \
  '{' \
  '  "schemaVersion": 1,' \
  '  "id": "dm",' \
  '  "name": "DM",' \
  "  \"version\": \"${driver_version}\"," \
  '  "driverClass": "dm.jdbc.driver.DmDriver",' \
  '  "artifacts": [' \
  '    {' \
  "      \"path\": \"${driver_filename}\"," \
  "      \"sha256\": \"${actual_sha256}\"" \
  '    }' \
  '  ]' \
  '}' > "${staging_directory}/driver-pack.json"
chmod 0644 "${staged_jar}" "${staging_directory}/driver-pack.json"

pack_directory="${output_root}/${pack_directory_name}"
if [[ -e "${pack_directory}" ]]; then
  if [[ ! -d "${pack_directory}" || -L "${pack_directory}" ]]; then
    echo "refusing to replace unsafe pack path: ${pack_directory}" >&2
    exit 1
  fi
  backup_directory="${output_root}/.${pack_directory_name}.previous.$$"
  if [[ -e "${backup_directory}" ]]; then
    echo "refusing to replace existing backup path: ${backup_directory}" >&2
    exit 1
  fi
  mv -- "${pack_directory}" "${backup_directory}"
fi

mv -- "${staging_directory}" "${pack_directory}"
staging_directory=""
if [[ -n "${backup_directory}" ]]; then
  rm -rf -- "${backup_directory}"
  backup_directory=""
fi

echo "Prepared DM JDBC ${driver_version} driver pack at ${output_root}"
echo "Set DM_TEST_DRIVER_PACK_DIR=${output_root} when running the DM product test"
