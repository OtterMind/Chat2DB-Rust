#!/usr/bin/env bash
set -euo pipefail

readonly h2_version="2.1.214"
readonly h2_filename="h2-${h2_version}.jar"
readonly h2_sha256="d623cdc0f61d218cf549a8d09f1c391ff91096116b22e2475475fce4fbe72bd0"
readonly h2_bytes="2543012"
readonly h2_url="https://repo.maven.apache.org/maven2/com/h2database/h2/${h2_version}/${h2_filename}"
readonly pack_directory_name="02-h2-migration"

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-${repository_root}/target/mysql-driver-packs}"
source_jar="${H2_MIGRATION_DRIVER_JAR:-}"
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
staged_jar="${staging_directory}/${h2_filename}"

if [[ -n "${source_jar}" ]]; then
  if [[ ! -f "${source_jar}" || -L "${source_jar}" ]]; then
    echo "H2_MIGRATION_DRIVER_JAR must be a non-symbolic regular file" >&2
    exit 1
  fi
  cp -- "${source_jar}" "${staged_jar}"
else
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required when H2_MIGRATION_DRIVER_JAR is not set" >&2
    exit 1
  fi
  curl --fail --location --silent --show-error \
    --retry 3 --retry-all-errors \
    --output "${staged_jar}" \
    "${h2_url}"
fi

actual_bytes="$(LC_ALL=C wc -c < "${staged_jar}" | tr -d '[:space:]')"
if [[ "${actual_bytes}" != "${h2_bytes}" ]]; then
  echo "H2 driver byte length mismatch: expected ${h2_bytes}, found ${actual_bytes}" >&2
  exit 1
fi

actual_sha256="$(sha256_file "${staged_jar}")"
if [[ "${actual_sha256}" != "${h2_sha256}" ]]; then
  echo "H2 driver SHA-256 mismatch: expected ${h2_sha256}, found ${actual_sha256}" >&2
  exit 1
fi

printf '%s\n' \
  '{' \
  '  "schemaVersion": 1,' \
  '  "id": "h2-legacy-migration",' \
  '  "name": "H2 legacy migration",' \
  "  \"version\": \"${h2_version}\"," \
  '  "driverClass": "org.h2.Driver",' \
  '  "artifacts": [' \
  '    {' \
  "      \"path\": \"${h2_filename}\"," \
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

echo "Prepared H2 ${h2_version} legacy-migration driver pack at ${output_root}"
