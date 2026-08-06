#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
target_root="${repository_root}/target"
output_root="${target_root}/macos-driver-packs"
staging_directory=""
previous_directory=""

cleanup() {
  if [[ -n "${staging_directory}" && -d "${staging_directory}" ]]; then
    rm -rf -- "${staging_directory}"
  fi
  if [[ -n "${previous_directory}" && -d "${previous_directory}" ]]; then
    if [[ ! -e "${output_root}" ]]; then
      mv -- "${previous_directory}" "${output_root}"
    else
      rm -rf -- "${previous_directory}"
    fi
  fi
}
trap cleanup EXIT

if [[ -e "${target_root}" && ( ! -d "${target_root}" || -L "${target_root}" ) ]]; then
  echo "refusing to use unsafe target directory: ${target_root}" >&2
  exit 1
fi
mkdir -p "${target_root}"

staging_directory="$(mktemp -d "${target_root}/.macos-driver-packs.staging.XXXXXX")"
"${repository_root}/scripts/prepare-mysql-driver-pack.sh" "${staging_directory}"
"${repository_root}/scripts/prepare-h2-driver-pack.sh" "${staging_directory}"

mysql_found=false
h2_found=false
entry_count=0
while IFS= read -r -d '' entry; do
  entry_count=$((entry_count + 1))
  entry_name="$(basename "${entry}")"
  if [[ ! -d "${entry}" || -L "${entry}" ]]; then
    echo "macOS driver-pack entry must be a non-symbolic directory: ${entry_name}" >&2
    exit 1
  fi
  case "${entry_name}" in
    01-mysql) mysql_found=true ;;
    02-h2-migration) h2_found=true ;;
    *)
      echo "macOS driver-pack staging contains an unauthorized entry: ${entry_name}" >&2
      exit 1
      ;;
  esac
done < <(find "${staging_directory}" -mindepth 1 -maxdepth 1 -print0)

if [[ "${entry_count}" -ne 2 || "${mysql_found}" != true || "${h2_found}" != true ]]; then
  echo "macOS driver packs must contain exactly 01-mysql and 02-h2-migration" >&2
  exit 1
fi

if [[ -e "${output_root}" ]]; then
  if [[ ! -d "${output_root}" || -L "${output_root}" ]]; then
    echo "refusing to replace unsafe macOS driver-pack root: ${output_root}" >&2
    exit 1
  fi
  previous_directory="${target_root}/.macos-driver-packs.previous.$$"
  if [[ -e "${previous_directory}" ]]; then
    echo "refusing to replace existing macOS driver-pack backup: ${previous_directory}" >&2
    exit 1
  fi
  mv -- "${output_root}" "${previous_directory}"
fi

mv -- "${staging_directory}" "${output_root}"
staging_directory=""
if [[ -n "${previous_directory}" ]]; then
  rm -rf -- "${previous_directory}"
  previous_directory=""
fi

echo "Prepared public macOS driver packs at ${output_root}"
