#!/usr/bin/env bash
set -euo pipefail

readonly connector_version="8.0.30"
readonly connector_filename="mysql-connector-java-${connector_version}.jar"
readonly connector_sha256="b5bf2f0987197c30adf74a9e419b89cda4c257da2d1142871f508416d5f2227a"
readonly connector_bytes="2513563"
readonly connector_url="https://repo.maven.apache.org/maven2/mysql/mysql-connector-java/${connector_version}/${connector_filename}"
readonly pack_directory_name="01-mysql"

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-${repository_root}/target/driver-packs}"
source_jar="${MYSQL_CONNECTOR_JAR:-}"
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
staged_jar="${staging_directory}/${connector_filename}"

if [[ -n "${source_jar}" ]]; then
  if [[ ! -f "${source_jar}" || -L "${source_jar}" ]]; then
    echo "MYSQL_CONNECTOR_JAR must be a non-symbolic regular file" >&2
    exit 1
  fi
  cp -- "${source_jar}" "${staged_jar}"
else
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required when MYSQL_CONNECTOR_JAR is not set" >&2
    exit 1
  fi
  curl --fail --location --silent --show-error \
    --retry 3 --retry-all-errors \
    --output "${staged_jar}" \
    "${connector_url}"
fi

actual_bytes="$(LC_ALL=C wc -c < "${staged_jar}" | tr -d '[:space:]')"
if [[ "${actual_bytes}" != "${connector_bytes}" ]]; then
  echo "MySQL Connector/J byte length mismatch: expected ${connector_bytes}, found ${actual_bytes}" >&2
  exit 1
fi

actual_sha256="$(sha256_file "${staged_jar}")"
if [[ "${actual_sha256}" != "${connector_sha256}" ]]; then
  echo "MySQL Connector/J SHA-256 mismatch: expected ${connector_sha256}, found ${actual_sha256}" >&2
  exit 1
fi

printf '%s\n' \
  '{' \
  '  "schemaVersion": 1,' \
  '  "id": "mysql",' \
  '  "name": "MySQL",' \
  '  "version": "8.0.30",' \
  '  "driverClass": "com.mysql.cj.jdbc.Driver",' \
  '  "artifacts": [' \
  '    {' \
  '      "path": "mysql-connector-java-8.0.30.jar",' \
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

echo "Prepared MySQL Connector/J ${connector_version} driver pack at ${output_root}"
echo "Set MYSQL_TEST_DRIVER_PACK_DIR=${output_root} when running the MySQL product test"
