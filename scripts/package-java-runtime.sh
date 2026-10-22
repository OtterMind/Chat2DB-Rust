#!/usr/bin/env bash
set -euo pipefail

# Packages the on-demand Java runtime archive: jlink runtime, compatibility
# engine, fixed Community classpath, and driver packs under one root layout.

if [[ "$#" -ne 6 ]]; then
  echo "usage: $0 <platform-id> <runtime-dir> <engine-jar> <classpath-dir> <driver-packs-dir> <output-dir>" >&2
  exit 1
fi

platform_id="$1"
runtime_directory="$2"
engine_jar="$3"
classpath_directory="$4"
driver_packs_directory="$5"
output_directory="$6"

case "${platform_id}" in
  macos-arm64 | macos-x64 | windows-x86_64 | linux-x86_64 | linux-arm64) ;;
  *)
    echo "unsupported Java runtime platform: ${platform_id}" >&2
    exit 1
    ;;
esac

for directory in "${runtime_directory}" "${classpath_directory}" "${driver_packs_directory}"; do
  if [[ ! -d "${directory}" || -L "${directory}" ]]; then
    echo "Java runtime input directory is missing or unsafe: ${directory}" >&2
    exit 1
  fi
done
if [[ ! -f "${engine_jar}" || -L "${engine_jar}" ]]; then
  echo "Java runtime engine JAR is missing or unsafe: ${engine_jar}" >&2
  exit 1
fi
if [[ ! -d "${output_directory}" || -L "${output_directory}" ]]; then
  echo "Java runtime output directory is missing or unsafe: ${output_directory}" >&2
  exit 1
fi

archive_name="chat2db-java-runtime-${platform_id}.zip"
archive_path="${output_directory}/${archive_name}"
staging_directory="$(mktemp -d "${TMPDIR:-/tmp}/chat2db-java-runtime.XXXXXX")"
cleanup() {
  rm -rf -- "${staging_directory}"
}
trap cleanup EXIT

cp -R "${runtime_directory}" "${staging_directory}/java"
mkdir -p "${staging_directory}/engine"
cp "${engine_jar}" "${staging_directory}/engine/chat2db-compat-runtime.jar"
cp -R "${classpath_directory}" "${staging_directory}/community-classpath"
cp -R "${driver_packs_directory}" "${staging_directory}/driver-packs"

if [[ "${platform_id}" == windows-* ]]; then
  java_binary="${staging_directory}/java/bin/java.exe"
else
  java_binary="${staging_directory}/java/bin/java"
fi
for required in "${java_binary}" "${staging_directory}/engine/chat2db-compat-runtime.jar"; do
  if [[ ! -f "${required}" ]]; then
    echo "Java runtime staging is missing ${required}" >&2
    exit 1
  fi
done

# -y keeps the JDK's internal symlinks, which the JVM needs on macOS and Linux.
(
  cd "${staging_directory}"
  if command -v zip >/dev/null 2>&1; then
    zip -q -r -y "${archive_path}" java engine community-classpath driver-packs
  elif command -v 7z >/dev/null 2>&1; then
    7z a -tzip -bso0 -bsp0 "${archive_path}" java engine community-classpath driver-packs
  elif command -v pwsh >/dev/null 2>&1; then
    pwsh -NoProfile -NonInteractive -Command \
      "Compress-Archive -Path 'java','engine','community-classpath','driver-packs' -DestinationPath '$(cygpath -w "${archive_path}")' -Force"
  else
    echo "no zip, 7z, or pwsh available to package the Java runtime" >&2
    exit 1
  fi
)

if command -v shasum >/dev/null 2>&1; then
  (cd "${output_directory}" && shasum -a 256 -- "${archive_name}" > "${archive_name}.sha256")
else
  (cd "${output_directory}" && sha256sum -- "${archive_name}" > "${archive_name}.sha256")
fi

echo "Packaged Java runtime: ${archive_path}"
