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
  "f275e08d774f839612374e991d09c5e6ea2d8b57"

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
echo "Verified self-contained macOS package at ${app_path}"
