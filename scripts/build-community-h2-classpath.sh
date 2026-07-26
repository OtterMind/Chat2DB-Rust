#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
community_root="${repository_root}/third_party/chat2db-community"
server_root="${community_root}/chat2db-community-server"
output_directory="${repository_root}/target/community-h2-classpath"
maven_wrapper="${repository_root}/java/mvnw"
maven_repository="${repository_root}/target/community-m2"
classpath_lock="${repository_root}/third_party/community-h2-classpath.lock"
classpath_lock_tool="${repository_root}/scripts/community-classpath-lock.sh"
classpath_sanitizer="${repository_root}/scripts/CommunityClasspathSanitizer.java"
expected_commit="f63cbf4a8334b45d9b1fbb268116e4dfc1fad1d7"
community_version="5.3.0"

if [[ ! -e "${community_root}/.git" ]]; then
  echo "Community source is missing; initialize third_party/chat2db-community" >&2
  exit 1
fi

actual_commit="$(git -C "${community_root}" rev-parse HEAD)"
if [[ "${actual_commit}" != "${expected_commit}" ]]; then
  echo "Community source must be pinned to ${expected_commit}; found ${actual_commit}" >&2
  exit 1
fi

if [[ -n "$(git -C "${community_root}" status --porcelain --untracked-files=all)" ]]; then
  echo "Community source must be clean before building the fixed compatibility classpath" >&2
  exit 1
fi

non_lf_checkout_count="$(
  git -C "${community_root}" ls-files --eol -- chat2db-community-server |
    LC_ALL=C awk '$1 == "i/lf" && $2 != "w/lf" { count++ } END { print count + 0 }'
)"
if [[ ! "${non_lf_checkout_count}" =~ ^[0-9]+$ ]]; then
  echo "Could not validate Community server checkout line endings" >&2
  exit 1
fi
if ((non_lf_checkout_count > 0)); then
  echo "Community server checkout contains ${non_lf_checkout_count} non-LF checkout(s) for LF source file(s)" >&2
  echo "Reinitialize the clean submodule with Git core.autocrlf=false before building" >&2
  exit 1
fi

community_output_timestamp="$(git -C "${community_root}" show -s --format=%ct "${expected_commit}")"
mkdir -p "${maven_repository}"

java "${classpath_sanitizer}" self-test

"${maven_wrapper}" -B \
  -Dmaven.repo.local="${maven_repository}" \
  -Dproject.build.outputTimestamp="${community_output_timestamp}" \
  -f "${server_root}/pom.xml" \
  -pl chat2db-community-plugins/chat2db-community-h2 \
  -am -DskipTests clean install

case "${output_directory}" in
  "${repository_root}/target/community-h2-classpath") ;;
  *)
    echo "refusing to replace unexpected output directory ${output_directory}" >&2
    exit 1
    ;;
esac
rm -rf -- "${output_directory}"
mkdir -p "${output_directory}"

"${maven_wrapper}" -B \
  -Dmaven.repo.local="${maven_repository}" \
  -Dproject.build.outputTimestamp="${community_output_timestamp}" \
  -f "${server_root}/pom.xml" \
  -pl chat2db-community-plugins/chat2db-community-h2 \
  dependency:copy-dependencies \
  -DincludeScope=runtime \
  -DexcludeGroupIds=com.h2database \
  -DoutputDirectory="${output_directory}"

cp \
  "${server_root}/chat2db-community-plugins/chat2db-community-h2/target/chat2db-community-h2-${community_version}.jar" \
  "${output_directory}/"

java "${classpath_sanitizer}" sanitize \
  "${output_directory}" \
  "${community_output_timestamp}"
java "${classpath_sanitizer}" verify "${output_directory}"

"${classpath_lock_tool}" verify \
  "${output_directory}" \
  "${classpath_lock}" \
  "${expected_commit}"

shopt -s nullglob
classpath_artifacts=("${output_directory}"/*.jar)
shopt -u nullglob
artifact_count="${#classpath_artifacts[@]}"
echo "Community H2 compatibility classpath: ${artifact_count} JARs at ${output_directory}"
