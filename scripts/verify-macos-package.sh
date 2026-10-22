#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_path="${1:-${repository_root}/target/macos-package-build/release/bundle/macos/Chat2DB Rust.app}"
resource_root="${app_path}/Contents/Resources/chat2db"
binary="${app_path}/Contents/MacOS/chat2db-desktop"

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
require_file "${resource_root}/licenses/Chat2DB-Rust-LICENSE.txt"
require_file "${resource_root}/licenses/Chat2DB-Community-LICENSE.txt"
require_file "${resource_root}/licenses/THIRD_PARTY_NOTICES.md"

if [[ ! -x "${binary}" ]]; then
  echo "packaged desktop binary must be executable" >&2
  exit 1
fi

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

# The Java runtime, compatibility engine, classpath, and driver packs are
# distributed as a separate on-demand archive. The app bundle must not carry
# them.
if [[ -e "${resource_root}/java" || -e "${resource_root}/engine" \
  || -e "${resource_root}/community-classpath" || -e "${resource_root}/driver-packs" ]]; then
  echo "desktop package must not embed the on-demand Java runtime" >&2
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
    if ! grep -Eq '^CodeDirectory .*[[:space:]]flags=.*\(runtime\)' <<<"${signature_details}"; then
      echo "${code_label} is missing the hardened runtime flag" >&2
      exit 1
    fi
    if [[ "${designated_requirement}" == *"cdhash"* ]]; then
      echo "${code_label} still has a build-specific cdhash requirement" >&2
      exit 1
    fi
  }

  verify_developer_id_code "${app_path}" "package" "${APPLE_TEAM_ID:-}"
fi
echo "Verified self-contained macOS package at ${app_path}"
