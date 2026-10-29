#!/usr/bin/env bash
set -euo pipefail

# Packages one JDBC driver pack as a standalone release asset. The archive keeps
# the runtime layout (driver-packs/<id>/...) so the desktop installer can extract
# it straight into an installed runtime.

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <pack-directory> <pack-id> <output-dir>" >&2
  exit 1
fi

pack_directory="$1"
pack_id="$2"
output_directory="$3"

if [[ ! -d "${pack_directory}" || -L "${pack_directory}" ]]; then
  echo "driver pack directory is missing or unsafe: ${pack_directory}" >&2
  exit 1
fi
if [[ ! -f "${pack_directory}/driver-pack.json" ]]; then
  echo "driver pack manifest is missing: ${pack_directory}/driver-pack.json" >&2
  exit 1
fi
if [[ ! -d "${output_directory}" || -L "${output_directory}" ]]; then
  echo "driver pack output directory is missing or unsafe: ${output_directory}" >&2
  exit 1
fi
# Absolute paths keep the archive writable after changing into the staging directory.
output_directory="$(cd "${output_directory}" && pwd -P)"
case "${pack_id}" in
  dm) ;;
  *)
    echo "unsupported driver pack id: ${pack_id}" >&2
    exit 1
    ;;
esac

archive_name="chat2db-driver-pack-${pack_id}.zip"
archive_path="${output_directory}/${archive_name}"
staging_directory="$(mktemp -d "${TMPDIR:-/tmp}/chat2db-driver-pack.XXXXXX")"
cleanup() {
  rm -rf -- "${staging_directory}"
}
trap cleanup EXIT

mkdir -p "${staging_directory}/driver-packs"
cp -R "${pack_directory}" "${staging_directory}/driver-packs/${pack_id}"
rm -f -- "${archive_path}"
(
  cd "${staging_directory}"
  if command -v zip >/dev/null 2>&1; then
    zip -q -r -y "${archive_path}" driver-packs
  else
    python3 -m zipfile -c "${archive_path}" driver-packs
  fi
)
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum -- "${archive_path}" | awk '{print $1}' > "${archive_path}.sha256"
else
  shasum -a 256 -- "${archive_path}" | awk '{print $1}' > "${archive_path}.sha256"
fi
echo "Packaged driver pack: ${archive_path}"
