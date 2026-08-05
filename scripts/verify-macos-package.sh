#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_path="${1:-${repository_root}/target/macos-package-build/release/bundle/macos/Chat2DB Rust.app}"
resource_root="${app_path}/Contents/Resources/chat2db"
java_bin="${resource_root}/java/bin/java"
engine_jar="${resource_root}/engine/chat2db-compat-runtime.jar"
community_classpath="${resource_root}/community-classpath"
driver_root="${resource_root}/driver-packs"
binary="${app_path}/Contents/MacOS/chat2db-desktop"
module_file="${repository_root}/packaging/macos/jlink-modules.txt"

require_file() {
  local path="$1"
  if [[ ! -f "${path}" || -L "${path}" ]]; then
    echo "required package file is missing or unsafe: ${path}" >&2
    exit 1
  fi
}

require_directory() {
  local path="$1"
  if [[ ! -d "${path}" || -L "${path}" ]]; then
    echo "required package directory is missing or unsafe: ${path}" >&2
    exit 1
  fi
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS package verification requires Darwin" >&2
  exit 1
fi
require_directory "${app_path}"
require_file "${binary}"
require_file "${java_bin}"
require_file "${engine_jar}"
require_directory "${community_classpath}"
require_directory "${driver_root}"
require_file "${resource_root}/licenses/Chat2DB-Rust-LICENSE.txt"
require_file "${resource_root}/licenses/Chat2DB-Community-LICENSE.txt"
require_file "${resource_root}/licenses/THIRD_PARTY_NOTICES.md"

if [[ ! -x "${binary}" || ! -x "${java_bin}" ]]; then
  echo "packaged desktop and Java binaries must be executable" >&2
  exit 1
fi

"${repository_root}/scripts/community-classpath-lock.sh" verify \
  "${community_classpath}" \
  "${repository_root}/third_party/community-h2-classpath.lock" \
  "3cb8af54cad5bd5caa20bb25f10d9b0e4f01931c"

driver_manifest="${driver_root}/01-mysql/driver-pack.json"
driver_jar="${driver_root}/01-mysql/mysql-connector-java-8.0.30.jar"
require_file "${driver_manifest}"
require_file "${driver_jar}"
expected_driver_sha="$(awk -F '"' '/"sha256"/ { print $4; exit }' "${driver_manifest}")"
actual_driver_sha="$(shasum -a 256 -- "${driver_jar}" | awk '{ print $1 }')"
if [[ -z "${expected_driver_sha}" || "${actual_driver_sha}" != "${expected_driver_sha}" ]]; then
  echo "packaged MySQL driver digest does not match its manifest" >&2
  exit 1
fi

h2_manifest="${driver_root}/02-h2-migration/driver-pack.json"
h2_jar="${driver_root}/02-h2-migration/h2-2.1.214.jar"
require_file "${h2_manifest}"
require_file "${h2_jar}"
expected_h2_sha="$(awk -F '"' '/"sha256"/ { print $4; exit }' "${h2_manifest}")"
actual_h2_sha="$(shasum -a 256 -- "${h2_jar}" | awk '{ print $1 }')"
if [[ -z "${expected_h2_sha}" || "${actual_h2_sha}" != "${expected_h2_sha}" ]]; then
  echo "packaged H2 migration driver digest does not match its manifest" >&2
  exit 1
fi

"${java_bin}" -version
while IFS= read -r module; do
  [[ -z "${module}" || "${module}" == \#* ]] && continue
  if ! "${java_bin}" --list-modules | cut -d@ -f1 | grep -Fxq "${module}"; then
    echo "packaged Java runtime is missing ${module}" >&2
    exit 1
  fi
done < "${module_file}"
"${java_bin}" -jar "${engine_jar}" < /dev/null

bundle_identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${app_path}/Contents/Info.plist")"
if [[ "${bundle_identifier}" != "ai.chat2db.desktop" ]]; then
  echo "unexpected bundle identifier: ${bundle_identifier}" >&2
  exit 1
fi
host_arch="$(uname -m)"
if ! lipo -archs "${binary}" | tr ' ' '\n' | grep -Fxq "${host_arch}"; then
  echo "desktop binary does not contain host architecture ${host_arch}" >&2
  exit 1
fi
if ! lipo -archs "${java_bin}" | tr ' ' '\n' | grep -Fxq "${host_arch}"; then
  echo "Java runtime does not contain host architecture ${host_arch}" >&2
  exit 1
fi

codesign --verify --deep --strict --verbose=2 "${app_path}"
if [[ "${CHAT2DB_REQUIRE_DEVELOPER_ID_SIGNATURE:-false}" == true ]]; then
  verify_developer_id_code() {
    local code_path="$1"
    local code_label="$2"
    local expected_team_id="$3"
    local signature_details
    local signing_team_id
    local signing_authority
    local signing_timestamp
    local designated_requirement

    codesign --verify --strict --verbose=2 "${code_path}"
    signature_details="$(codesign -dv --verbose=4 "${code_path}" 2>&1)"
    signing_team_id="$(awk -F= '/^TeamIdentifier=/ { print $2; exit }' <<<"${signature_details}")"
    signing_authority="$(awk -F= '/^Authority=/ { print $2; exit }' <<<"${signature_details}")"
    signing_timestamp="$(awk -F= '/^Timestamp=/ { print $2; exit }' <<<"${signature_details}")"
    designated_requirement="$(codesign -d -r- "${code_path}" 2>&1)"
    if [[ "${signing_authority}" != Developer\ ID\ Application:* ]]; then
      echo "${code_label} is not signed by a Developer ID Application identity" >&2
      exit 1
    fi
    if [[ -z "${signing_team_id}" || "${signing_team_id}" == "not set" ]]; then
      echo "${code_label} is missing a TeamIdentifier" >&2
      exit 1
    fi
    if [[ -n "${expected_team_id}" && "${signing_team_id}" != "${expected_team_id}" ]]; then
      echo "${code_label} TeamIdentifier does not match APPLE_TEAM_ID" >&2
      exit 1
    fi
    if [[ -z "${signing_timestamp}" || "${signing_timestamp}" == "none" ]]; then
      echo "${code_label} is missing a trusted timestamp" >&2
      exit 1
    fi
    if ! grep -Eq '^flags=.*\(runtime\)' <<<"${signature_details}"; then
      echo "${code_label} is missing the hardened runtime flag" >&2
      exit 1
    fi
    if [[ "${designated_requirement}" == *"cdhash"* ]]; then
      echo "${code_label} still has a build-specific cdhash requirement" >&2
      exit 1
    fi
  }

  verify_developer_id_code "${app_path}" "package" "${APPLE_TEAM_ID:-}"

  runtime_macho_count=0
  while IFS= read -r -d '' runtime_file; do
    if [[ "$(file -b "${runtime_file}")" != *"Mach-O"* ]]; then
      continue
    fi
    runtime_macho_count=$((runtime_macho_count + 1))
    verify_developer_id_code "${runtime_file}" "packaged Java code ${runtime_file#"${app_path}/"}" ""
  done < <(find "${resource_root}/java" -type f -print0)
  if [[ "${runtime_macho_count}" -eq 0 ]]; then
    echo "packaged Java runtime contains no Mach-O code" >&2
    exit 1
  fi
  echo "Verified ${runtime_macho_count} Developer ID signed Java runtime binaries"
fi
echo "Verified self-contained macOS package at ${app_path}"
