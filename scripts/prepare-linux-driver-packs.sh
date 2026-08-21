#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
target_root="${repository_root}/target"
output_root="${target_root}/linux-driver-packs"
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
staging_directory="$(mktemp -d "${target_root}/.linux-driver-packs.staging.XXXXXX")"
"${repository_root}/scripts/prepare-mysql-driver-pack.sh" "${staging_directory}"
"${repository_root}/scripts/prepare-h2-driver-pack.sh" "${staging_directory}"

entry_count=0
while IFS= read -r -d '' entry; do
  entry_count=$((entry_count + 1))
  entry_name="$(basename "${entry}")"
  if [[ ! -d "${entry}" || -L "${entry}" ]]; then
    echo "Linux driver-pack entry must be a non-symbolic directory: ${entry_name}" >&2
    exit 1
  fi
  case "${entry_name}" in
    01-mysql|02-h2-migration) ;;
    *) echo "unauthorized Linux driver-pack entry: ${entry_name}" >&2; exit 1 ;;
  esac
done < <(find "${staging_directory}" -mindepth 1 -maxdepth 1 -print0)
if [[ "${entry_count}" -ne 2 ]]; then
  echo "Linux driver packs must contain exactly 01-mysql and 02-h2-migration" >&2
  exit 1
fi

if [[ -e "${output_root}" ]]; then
  if [[ ! -d "${output_root}" || -L "${output_root}" ]]; then
    echo "refusing to replace unsafe Linux driver-pack root: ${output_root}" >&2
    exit 1
  fi
  previous_directory="${target_root}/.linux-driver-packs.previous.$$"
  mv -- "${output_root}" "${previous_directory}"
fi
mv -- "${staging_directory}" "${output_root}"
staging_directory=""
if [[ -n "${previous_directory}" ]]; then
  rm -rf -- "${previous_directory}"
  previous_directory=""
fi

echo "Prepared Linux driver packs at ${output_root}"
