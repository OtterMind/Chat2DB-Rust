#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
module_file="${repository_root}/packaging/macos/jlink-modules.txt"
target_root="${repository_root}/target"
output_directory="${target_root}/linux-runtime"
staging_directory=""
backup_directory=""

cleanup() {
  if [[ -n "${staging_directory}" && -d "${staging_directory}" ]]; then
    rm -rf -- "${staging_directory}"
  fi
  if [[ -n "${backup_directory}" && -d "${backup_directory}" ]]; then
    if [[ ! -e "${output_directory}" ]]; then
      mv -- "${backup_directory}" "${output_directory}"
    else
      rm -rf -- "${backup_directory}"
    fi
  fi
}
trap cleanup EXIT

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux jlink runtime generation requires Linux" >&2
  exit 1
fi
if [[ -e "${target_root}" && ( ! -d "${target_root}" || -L "${target_root}" ) ]]; then
  echo "refusing to use unsafe target directory: ${target_root}" >&2
  exit 1
fi
mkdir -p "${target_root}"

java_home="${JAVA_HOME:-}"
if [[ -z "${java_home}" ]]; then
  java_home="$(dirname "$(dirname "$(command -v java)")")"
fi
if [[ ! -x "${java_home}/bin/java" || ! -x "${java_home}/bin/jlink" || ! -d "${java_home}/jmods" ]]; then
  echo "JAVA_HOME must point to a JDK 17 containing java, jlink, and jmods" >&2
  exit 1
fi
java_major="$("${java_home}/bin/java" -version 2>&1 | awk -F '[\".]' '/version/ { print $2; exit }')"
if [[ "${java_major}" != "17" ]]; then
  echo "Linux runtime must be built with JDK 17; found ${java_major:-unknown}" >&2
  exit 1
fi

modules="$(awk '/^[[:space:]]*#/ || /^[[:space:]]*$/ { next } { gsub(/[[:space:]]/, ""); print }' "${module_file}" | LC_ALL=C sort -u | paste -sd, -)"
if [[ -z "${modules}" ]]; then
  echo "jlink module list is empty" >&2
  exit 1
fi

staging_directory="$(mktemp -d "${target_root}/.linux-runtime.staging.XXXXXX")"
rm -rf -- "${staging_directory}"
"${java_home}/bin/jlink" \
  --module-path "${java_home}/jmods" \
  --add-modules "${modules}" \
  --bind-services \
  --strip-debug \
  --no-header-files \
  --no-man-pages \
  --compress=2 \
  --output "${staging_directory}"
chmod -R u+rwX,go+rX "${staging_directory}"
"${staging_directory}/bin/java" -version

if [[ -e "${output_directory}" ]]; then
  if [[ ! -d "${output_directory}" || -L "${output_directory}" ]]; then
    echo "refusing to replace unsafe runtime path: ${output_directory}" >&2
    exit 1
  fi
  backup_directory="${target_root}/.linux-runtime.previous.$$"
  mv -- "${output_directory}" "${backup_directory}"
fi
mv -- "${staging_directory}" "${output_directory}"
staging_directory=""
if [[ -n "${backup_directory}" ]]; then
  rm -rf -- "${backup_directory}"
  backup_directory=""
fi

echo "Built Linux Java 17 runtime at ${output_directory}"
