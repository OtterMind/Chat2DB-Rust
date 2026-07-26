#!/usr/bin/env bash
set -euo pipefail

readonly lock_format_version="1"
readonly expected_artifact_count="148"

usage() {
  echo "usage: $0 <generate|verify> <classpath-directory> <lock-file> <source-commit>" >&2
  exit 64
}

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
    echo "cannot hash Community classpath: sha256sum, shasum, or openssl is required" >&2
    return 1
  fi

  digest="${output%% *}"
  digest="$(printf '%s' "${digest}" | tr '[:upper:]' '[:lower:]')"
  if [[ ! "${digest}" =~ ^[0-9a-f]{64}$ ]]; then
    echo "invalid SHA-256 output while hashing ${path}" >&2
    return 1
  fi
  printf '%s' "${digest}"
}

manifest_contents() {
  local classpath_directory="$1"
  local source_commit="$2"
  local entry
  local filename
  local byte_length
  local digest
  local -a entries
  local -a records=()

  if [[ ! "${source_commit}" =~ ^[0-9a-f]{40}$ ]]; then
    echo "Community source commit must be a 40-character lowercase Git SHA" >&2
    return 1
  fi
  if [[ ! -d "${classpath_directory}" || -L "${classpath_directory}" ]]; then
    echo "Community classpath must be a non-symbolic directory: ${classpath_directory}" >&2
    return 1
  fi

  shopt -s nullglob dotglob
  entries=("${classpath_directory}"/*)
  shopt -u nullglob dotglob
  if [[ "${#entries[@]}" -ne "${expected_artifact_count}" ]]; then
    echo "Community classpath must contain exactly ${expected_artifact_count} JARs; found ${#entries[@]}" >&2
    return 1
  fi

  for entry in "${entries[@]}"; do
    filename="${entry##*/}"
    if [[ -L "${entry}" || ! -f "${entry}" || ! "${filename}" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*\.jar$ ]]; then
      echo "Community classpath contains an unexpected entry: ${entry}" >&2
      return 1
    fi
    byte_length="$(LC_ALL=C wc -c < "${entry}" | tr -d '[:space:]')"
    if [[ ! "${byte_length}" =~ ^[0-9]+$ ]]; then
      echo "cannot determine byte length for Community classpath artifact ${entry}" >&2
      return 1
    fi
    digest="$(sha256_file "${entry}")"
    records+=("artifact"$'\t'"${filename}"$'\t'"${digest}"$'\t'"${byte_length}")
  done

  printf 'format_version\t%s\n' "${lock_format_version}"
  printf 'source_commit\t%s\n' "${source_commit}"
  printf 'artifact_count\t%s\n' "${expected_artifact_count}"
  printf '%s\n' "${records[@]}" | LC_ALL=C sort
}

generate_lock() {
  local classpath_directory="$1"
  local lock_file="$2"
  local source_commit="$3"
  local lock_directory
  local temporary_lock

  lock_directory="$(dirname "${lock_file}")"
  if [[ ! -d "${lock_directory}" || -L "${lock_directory}" || -L "${lock_file}" ]]; then
    echo "Community classpath lock path is unsafe: ${lock_file}" >&2
    return 1
  fi
  temporary_lock="$(mktemp "${lock_file}.tmp.XXXXXX")"
  if ! manifest_contents "${classpath_directory}" "${source_commit}" > "${temporary_lock}"; then
    rm -f -- "${temporary_lock}"
    return 1
  fi
  chmod 0644 "${temporary_lock}"
  if ! mv -f -- "${temporary_lock}" "${lock_file}"; then
    rm -f -- "${temporary_lock}"
    return 1
  fi
  echo "Wrote Community classpath lock: ${lock_file}"
}

verify_lock() {
  local classpath_directory="$1"
  local lock_file="$2"
  local source_commit="$3"
  local actual_lock

  if [[ ! -f "${lock_file}" || -L "${lock_file}" ]]; then
    echo "Community classpath lock is missing or unsafe: ${lock_file}" >&2
    return 1
  fi
  actual_lock="$(mktemp "${TMPDIR:-/tmp}/chat2db-community-classpath-lock.XXXXXX")"
  if ! manifest_contents "${classpath_directory}" "${source_commit}" > "${actual_lock}"; then
    rm -f -- "${actual_lock}"
    return 1
  fi
  if ! cmp -s -- "${lock_file}" "${actual_lock}"; then
    echo "Community classpath does not match ${lock_file}" >&2
    diff -u -- "${lock_file}" "${actual_lock}" >&2 || true
    rm -f -- "${actual_lock}"
    return 1
  fi
  rm -f -- "${actual_lock}"
}

[[ "$#" -eq 4 ]] || usage
operation="$1"
classpath_directory="$2"
lock_file="$3"
source_commit="$4"

case "${operation}" in
  generate)
    generate_lock "${classpath_directory}" "${lock_file}" "${source_commit}"
    ;;
  verify)
    verify_lock "${classpath_directory}" "${lock_file}" "${source_commit}"
    ;;
  *)
    usage
    ;;
esac
