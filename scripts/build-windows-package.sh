#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
desktop_root="${repository_root}/apps/chat2db-desktop"
build_target="${CHAT2DB_WINDOWS_BUILD_TARGET:-${repository_root}/target/windows-package-build}"
package_directory="${repository_root}/target/windows-package"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) ;;
  *)
    echo "Windows package generation requires a Windows runner" >&2
    exit 1
    ;;
esac

target_root="${repository_root}/target"
if [[ -e "${target_root}" && ( ! -d "${target_root}" || -L "${target_root}" ) ]]; then
  echo "refusing to use unsafe target directory: ${target_root}" >&2
  exit 1
fi
mkdir -p "${target_root}"
if [[ "${build_target}" != /* || "$(dirname "${build_target}")" != "${target_root}" ]]; then
  echo "CHAT2DB_WINDOWS_BUILD_TARGET must be an absolute direct child of ${target_root}" >&2
  exit 1
fi
case "$(basename "${build_target}")" in
  windows-package-build|windows-package-build-*) ;;
  *)
    echo "CHAT2DB_WINDOWS_BUILD_TARGET must use the windows-package-build name prefix" >&2
    exit 1
    ;;
esac
if [[ -L "${build_target}" || ( -e "${build_target}" && ! -d "${build_target}" ) ]]; then
  echo "refusing to use unsafe Windows build target: ${build_target}" >&2
  exit 1
fi

for path in \
  "${repository_root}/target/windows-runtime/bin/java.exe" \
  "${repository_root}/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
  "${repository_root}/target/community-h2-classpath" \
  "${repository_root}/target/windows-driver-packs" \
  "${repository_root}/apps/frontend/dist"; do
  if [[ ! -e "${path}" ]]; then
    echo "package prerequisite is missing: ${path}" >&2
    exit 1
  fi
done

if ! cargo tauri --version | grep -Fxq "tauri-cli 2.8.4"; then
  echo "tauri-cli 2.8.4 is required; install it with cargo install tauri-cli --version 2.8.4 --locked" >&2
  exit 1
fi
rust_toolchain="${CHAT2DB_RUST_TOOLCHAIN:-1.88.0}"
rust_version="$(rustup run "${rust_toolchain}" rustc --version 2>/dev/null || true)"
if [[ "${rust_version}" != rustc\ 1.88.0\ * ]]; then
  echo "Rust 1.88.0 is required for Windows packaging; found ${rust_version:-missing}" >&2
  exit 1
fi

rm -rf -- "${build_target}"
mkdir -p "${build_target}"
(
  cd "${desktop_root}"
  CARGO_TARGET_DIR="${build_target}" \
  RUSTUP_TOOLCHAIN="${rust_toolchain}" \
  CI=true cargo tauri build \
    --verbose \
    --config tauri.windows.package.conf.json \
    --bundles nsis,msi \
    --ci \
    --ignore-version-mismatches \
    -- \
    --locked
)

bundle_root="${build_target}/release/bundle"
nsis_artifacts=("${bundle_root}/nsis"/*.exe)
msi_artifacts=("${bundle_root}/msi"/*.msi)
if [[ ! -f "${nsis_artifacts[0]}" || ! -f "${msi_artifacts[0]}" ]]; then
  echo "Tauri did not create both NSIS and MSI artifacts under ${bundle_root}" >&2
  exit 1
fi

case "${package_directory}" in
  "${repository_root}/target/windows-package") ;;
  *)
    echo "refusing to replace unexpected package directory: ${package_directory}" >&2
    exit 1
    ;;
esac
rm -rf -- "${package_directory}"
mkdir -p "${package_directory}"
cp -- "${nsis_artifacts[0]}" "${package_directory}/"
cp -- "${msi_artifacts[0]}" "${package_directory}/"
(
  cd "${package_directory}"
  sha256sum ./*.exe ./*.msi > SHA256SUMS
  {
    echo "Chat2DB Rust Windows test package"
    echo "architecture=x86_64"
    echo "target=windows"
    echo "rust_toolchain=${rust_toolchain}"
    echo "tauri_cli=$(cargo tauri --version)"
    echo "nsis=$(basename "${nsis_artifacts[0]}")"
    echo "msi=$(basename "${msi_artifacts[0]}")"
  } > BUILD-MANIFEST.txt
)

echo "Built Windows installers at ${package_directory}"
echo "Checksums: ${package_directory}/SHA256SUMS"
