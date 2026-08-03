#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_tool="${repository_root}/scripts/build-community-h2-classpath.sh"
lock_tool="${repository_root}/scripts/community-classpath-lock.sh"
sanitizer="${repository_root}/scripts/CommunityClasspathSanitizer.java"
output_directory="${repository_root}/target/community-h2-classpath"
expected_commit="3cb8af54cad5bd5caa20bb25f10d9b0e4f01931c"
first_lock="$(mktemp "${TMPDIR:-/tmp}/chat2db-community-first-lock.XXXXXX")"
second_lock="$(mktemp "${TMPDIR:-/tmp}/chat2db-community-second-lock.XXXXXX")"

cleanup() {
  rm -f -- "${first_lock}" "${second_lock}"
}
trap cleanup EXIT

"${build_tool}"
"${lock_tool}" generate "${output_directory}" "${first_lock}" "${expected_commit}"
first_digest="$(java "${sanitizer}" sha256 "${first_lock}")"

"${build_tool}"
"${lock_tool}" generate "${output_directory}" "${second_lock}" "${expected_commit}"
second_digest="$(java "${sanitizer}" sha256 "${second_lock}")"

if ! cmp -s -- "${first_lock}" "${second_lock}"; then
  echo "consecutive clean Community classpath builds were not byte-reproducible" >&2
  diff -u -- "${first_lock}" "${second_lock}" >&2 || true
  exit 1
fi

echo "First clean classpath manifest SHA-256:  ${first_digest}"
echo "Second clean classpath manifest SHA-256: ${second_digest}"
echo "Consecutive clean Community classpath builds are byte-reproducible"
