#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
module_file="${repository_root}/packaging/macos/jlink-modules.txt"
output_directory="${repository_root}/target/macos-runtime"
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

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS jlink runtime generation requires Darwin" >&2
  exit 1
fi
target_root="${repository_root}/target"
if [[ -e "${target_root}" && ( ! -d "${target_root}" || -L "${target_root}" ) ]]; then
  echo "refusing to use unsafe target directory: ${target_root}" >&2
  exit 1
fi
mkdir -p "${target_root}"
git_directory="$(git -C "${repository_root}" rev-parse --absolute-git-dir)"
lock_file="${git_directory}/chat2db-macos-package.lock"
if [[ -L "${lock_file}" || ( -e "${lock_file}" && ! -f "${lock_file}" ) ]]; then
  echo "refusing to use unsafe macOS package lock: ${lock_file}" >&2
  exit 1
fi
exec 9>"${lock_file}"
if ! lockf -s -t 0 9; then
  echo "another macOS runtime or package build is already using this checkout" >&2
  exit 1
fi
if [[ ! -f "${module_file}" || -L "${module_file}" ]]; then
  echo "jlink module list must be a non-symbolic regular file: ${module_file}" >&2
  exit 1
fi

java_home="${JAVA_HOME:-}"
if [[ -z "${java_home}" ]]; then
  java_home="$(/usr/libexec/java_home -v 17)"
fi
if [[ ! -x "${java_home}/bin/java" || ! -x "${java_home}/bin/jlink" ]]; then
  echo "JAVA_HOME must point to a JDK 17 containing java and jlink" >&2
  exit 1
fi
java_major="$("${java_home}/bin/java" -version 2>&1 | awk -F '[\".]' '/version/ { print $2; exit }')"
if [[ "${java_major}" != "17" ]]; then
  echo "macOS runtime must be built with JDK 17; found ${java_major:-unknown}" >&2
  exit 1
fi

modules="$(
  awk '
    /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
    { gsub(/[[:space:]]/, ""); print }
  ' "${module_file}" | LC_ALL=C sort -u | paste -sd, -
)"
if [[ -z "${modules}" ]]; then
  echo "jlink module list is empty" >&2
  exit 1
fi

case "${output_directory}" in
  "${repository_root}/target/macos-runtime") ;;
  *)
    echo "refusing to replace unexpected runtime directory: ${output_directory}" >&2
    exit 1
    ;;
esac
staging_directory="$(mktemp -d "${target_root}/.macos-runtime.staging.XXXXXX")"
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

# Tauri refreshes bundled resources in-place on incremental builds. jlink
# emits read-only legal files, so normalize the tree before it is staged.
chmod -R u+rwX,go+rX "${staging_directory}"

"${staging_directory}/bin/java" -version
while IFS= read -r module; do
  [[ -z "${module}" || "${module}" == \#* ]] && continue
  if ! "${staging_directory}/bin/java" --list-modules | cut -d@ -f1 | grep -Fxq "${module}"; then
    echo "generated runtime is missing required module ${module}" >&2
    exit 1
  fi
done < "${module_file}"

if [[ -e "${output_directory}" ]]; then
  if [[ ! -d "${output_directory}" || -L "${output_directory}" ]]; then
    echo "refusing to replace unsafe runtime path: ${output_directory}" >&2
    exit 1
  fi
  backup_directory="${repository_root}/target/.macos-runtime.previous.$$"
  if [[ -e "${backup_directory}" ]]; then
    echo "refusing to replace existing runtime backup: ${backup_directory}" >&2
    exit 1
  fi
  mv -- "${output_directory}" "${backup_directory}"
fi
mv -- "${staging_directory}" "${output_directory}"
staging_directory=""
if [[ -n "${backup_directory}" ]]; then
  rm -rf -- "${backup_directory}"
  backup_directory=""
fi

echo "Built macOS Java 17 runtime at ${output_directory}"
