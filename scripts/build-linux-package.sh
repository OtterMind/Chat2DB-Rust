#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
desktop_root="${repository_root}/apps/chat2db-desktop"
build_target="${CHAT2DB_LINUX_BUILD_TARGET:-${repository_root}/target/linux-package-build}"
package_directory="${repository_root}/target/linux-package"
license_resource_directory="${repository_root}/target/linux-license-resources"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux package generation requires a Linux runner" >&2
  exit 1
fi
target_root="${repository_root}/target"
if [[ -e "${target_root}" && ( ! -d "${target_root}" || -L "${target_root}" ) ]]; then
  echo "refusing to use unsafe target directory: ${target_root}" >&2
  exit 1
fi
mkdir -p "${target_root}"
if [[ "${build_target}" != /* || "$(dirname "${build_target}")" != "${target_root}" ]]; then
  echo "CHAT2DB_LINUX_BUILD_TARGET must be an absolute direct child of ${target_root}" >&2
  exit 1
fi
case "$(basename "${build_target}")" in
  linux-package-build|linux-package-build-*) ;;
  *) echo "CHAT2DB_LINUX_BUILD_TARGET must use the linux-package-build name prefix" >&2; exit 1 ;;
esac

rm -rf -- "${license_resource_directory}"
mkdir -p -- "${license_resource_directory}"
cp -- "${repository_root}/LICENSE" "${license_resource_directory}/Chat2DB-Rust-LICENSE.txt"
cp -- "${repository_root}/third_party/chat2db-community/LICENSE" \
  "${license_resource_directory}/Chat2DB-Community-LICENSE.txt"

for path in \
  "${repository_root}/target/linux-runtime/bin/java" \
  "${repository_root}/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
  "${repository_root}/target/community-h2-classpath" \
  "${repository_root}/target/linux-driver-packs" \
  "${license_resource_directory}/Chat2DB-Rust-LICENSE.txt" \
  "${license_resource_directory}/Chat2DB-Community-LICENSE.txt" \
  "${repository_root}/apps/frontend/dist"; do
  if [[ ! -e "${path}" ]]; then
    echo "package prerequisite is missing: ${path}" >&2
    exit 1
  fi
done

if ! cargo tauri --version | grep -Fxq "tauri-cli 2.8.4"; then
  echo "tauri-cli 2.8.4 is required" >&2
  exit 1
fi
rust_toolchain="${CHAT2DB_RUST_TOOLCHAIN:-1.88.0}"
rust_version="$(rustup run "${rust_toolchain}" rustc --version 2>/dev/null || true)"
if [[ "${rust_version}" != rustc\ 1.88.0\ * ]]; then
  echo "Rust 1.88.0 is required; found ${rust_version:-missing}" >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64) appimage_arch="x86_64" ;;
  aarch64) appimage_arch="aarch64" ;;
  *)
    echo "unsupported Linux package architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

rm -rf -- "${build_target}"
mkdir -p "${build_target}"
(
  cd "${desktop_root}"
  # GitHub's ARM64 runners do not expose FUSE; force AppImage tools to extract
  # themselves instead of mounting their AppImage runtime.
  # linuxdeploy's bundled strip cannot handle modern RELR relocations in
  # Ubuntu's WebKitGTK libraries; keep those libraries intact in AppImage.
  # libjvm.so is already supplied by the bundled jlink runtime. It is a
  # private dependency of libjawt.so and must not be resolved as a system lib.
  APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true \
    LINUXDEPLOY_EXCLUDED_LIBRARIES="libjvm.so" \
    LD_LIBRARY_PATH="${repository_root}/target/linux-runtime/lib/server:${repository_root}/target/linux-runtime/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}" \
    ARCH="${appimage_arch}" \
    CARGO_TARGET_DIR="${build_target}" RUSTUP_TOOLCHAIN="${rust_toolchain}" CI=true \
    cargo tauri build --verbose --config tauri.linux.package.conf.json \
      --bundles appimage,deb,rpm --ci --ignore-version-mismatches -- --locked
)

bundle_root="${build_target}/release/bundle"
appimage_artifacts=("${bundle_root}/appimage"/*.AppImage)
deb_artifacts=("${bundle_root}/deb"/*.deb)
rpm_artifacts=("${bundle_root}/rpm"/*.rpm)
if [[ ! -f "${appimage_artifacts[0]}" || ! -f "${deb_artifacts[0]}" || ! -f "${rpm_artifacts[0]}" ]]; then
  echo "Tauri did not create AppImage, deb, and rpm artifacts under ${bundle_root}" >&2
  exit 1
fi

case "${package_directory}" in
  "${repository_root}/target/linux-package") ;;
  *) echo "refusing to replace unexpected package directory: ${package_directory}" >&2; exit 1 ;;
esac
rm -rf -- "${package_directory}"
mkdir -p "${package_directory}"
cp -- "${appimage_artifacts[0]}" "${deb_artifacts[0]}" "${rpm_artifacts[0]}" "${package_directory}/"
(
  cd "${package_directory}"
  sha256sum ./*.AppImage ./*.deb ./*.rpm > SHA256SUMS
  {
    echo "Chat2DB Rust Linux test package"
    echo "architecture=$(uname -m)"
    echo "target=linux"
    echo "rust_toolchain=${rust_toolchain}"
    echo "tauri_cli=$(cargo tauri --version)"
    echo "appimage=$(basename "${appimage_artifacts[0]}")"
    echo "deb=$(basename "${deb_artifacts[0]}")"
    echo "rpm=$(basename "${rpm_artifacts[0]}")"
  } > BUILD-MANIFEST.txt
)

echo "Built Linux packages at ${package_directory}"
