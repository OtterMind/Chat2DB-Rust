#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
desktop_root="${repository_root}/apps/chat2db-desktop"
build_target="${CHAT2DB_MACOS_BUILD_TARGET:-${repository_root}/target/macos-package-build}"
app_path="${build_target}/release/bundle/macos/Chat2DB Rust.app"
package_directory="${repository_root}/target/macos-package"
staging_directory=""

cleanup() {
  if [[ -n "${staging_directory}" && -d "${staging_directory}" ]]; then
    rm -rf -- "${staging_directory}"
  fi
}
trap cleanup EXIT

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS package generation requires Darwin" >&2
  exit 1
fi
target_root="${repository_root}/target"
if [[ -e "${target_root}" && ( ! -d "${target_root}" || -L "${target_root}" ) ]]; then
  echo "refusing to use unsafe target directory: ${target_root}" >&2
  exit 1
fi
mkdir -p "${target_root}"
if [[ "${build_target}" != /* || "$(dirname "${build_target}")" != "${target_root}" ]]; then
  echo "CHAT2DB_MACOS_BUILD_TARGET must be an absolute direct child of ${target_root}" >&2
  exit 1
fi
case "$(basename "${build_target}")" in
  macos-package-build | macos-package-build-*) ;;
  *)
    echo "CHAT2DB_MACOS_BUILD_TARGET must use the macos-package-build name prefix" >&2
    exit 1
    ;;
esac
if [[ -L "${build_target}" || ( -e "${build_target}" && ! -d "${build_target}" ) ]]; then
  echo "refusing to use unsafe macOS build target: ${build_target}" >&2
  exit 1
fi
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
host_arch="$(uname -m)"
case "${host_arch}" in
  arm64) artifact_arch="aarch64" ;;
  x86_64) artifact_arch="x86_64" ;;
  *)
    echo "unsupported macOS architecture: ${host_arch}" >&2
    exit 1
    ;;
esac

for path in \
  "${repository_root}/target/macos-runtime/bin/java" \
  "${repository_root}/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
  "${repository_root}/target/community-h2-classpath" \
  "${repository_root}/target/mysql-driver-packs" \
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
  echo "Rust 1.88.0 is required for macOS packaging; found ${rust_version:-missing}" >&2
  exit 1
fi

staged_resource_root="${build_target}/release/chat2db"
if [[ -L "${staged_resource_root}" || ( -e "${staged_resource_root}" && ! -d "${staged_resource_root}" ) ]]; then
  echo "refusing to refresh unsafe staged resource directory: ${staged_resource_root}" >&2
  exit 1
fi
if [[ -d "${staged_resource_root}" ]]; then
  chmod -R u+rwX "${staged_resource_root}"
fi

(
  cd "${desktop_root}"
  CARGO_TARGET_DIR="${build_target}" \
  RUSTUP_TOOLCHAIN="${rust_toolchain}" \
  CI=true cargo tauri build \
    --config tauri.package.conf.json \
    --bundles app \
    --ci \
    --ignore-version-mismatches \
    -- \
    --locked
)

if [[ ! -d "${app_path}" || -L "${app_path}" ]]; then
  echo "Tauri did not create the expected app bundle: ${app_path}" >&2
  exit 1
fi

signing_identity="${APPLE_SIGNING_IDENTITY:--}"
if [[ "${signing_identity}" == "-" ]]; then
  codesign --force --deep --sign - --timestamp=none "${app_path}"
else
  codesign --force --deep --options runtime --timestamp --sign "${signing_identity}" "${app_path}"
fi
"${repository_root}/scripts/verify-macos-package.sh" "${app_path}"

version="$(awk '
  /^\[workspace.package\]$/ { in_package = 1; next }
  /^\[/ { in_package = 0 }
  in_package && /^version[[:space:]]*=/ {
    gsub(/[[:space:]\"]/ , "", $0)
    sub(/^version=/, "", $0)
    print
    exit
  }
' "${repository_root}/Cargo.toml")"
if [[ -z "${version}" ]]; then
  echo "could not resolve workspace package version" >&2
  exit 1
fi

case "${package_directory}" in
  "${repository_root}/target/macos-package") ;;
  *)
    echo "refusing to replace unexpected package directory: ${package_directory}" >&2
    exit 1
    ;;
esac
rm -rf -- "${package_directory}"
mkdir -p "${package_directory}"

artifact_base="Chat2DB-Rust_${version}_${artifact_arch}"
zip_path="${package_directory}/${artifact_base}.app.zip"
dmg_path="${package_directory}/${artifact_base}.dmg"
ditto -c -k --sequesterRsrc --keepParent "${app_path}" "${zip_path}"

staging_directory="$(mktemp -d "${repository_root}/target/.macos-dmg.staging.XXXXXX")"
ditto "${app_path}" "${staging_directory}/Chat2DB Rust.app"
ln -s /Applications "${staging_directory}/Applications"
hdiutil create \
  -volname "Chat2DB Rust" \
  -srcfolder "${staging_directory}" \
  -ov \
  -format UDZO \
  "${dmg_path}"
rm -rf -- "${staging_directory}"
staging_directory=""

(
  cd "${package_directory}"
  shasum -a 256 -- "$(basename "${zip_path}")" "$(basename "${dmg_path}")" > SHA256SUMS
)
git_commit="$(git -C "${repository_root}" rev-parse HEAD)"
community_commit="$(git -C "${repository_root}/third_party/chat2db-community" rev-parse HEAD)"
app_kib="$(du -sk "${app_path}" | awk '{ print $1 }')"
cat > "${package_directory}/BUILD-MANIFEST.txt" <<EOF
Chat2DB Rust macOS test package
version=${version}
architecture=${artifact_arch}
git_commit=${git_commit}
community_commit=${community_commit}
app_size_kib=${app_kib}
signing_identity=${signing_identity}
distribution_status=internal-test-only
EOF

echo "Built self-contained macOS app: ${app_path}"
echo "Packaged ZIP: ${zip_path}"
echo "Packaged DMG: ${dmg_path}"
echo "Checksums: ${package_directory}/SHA256SUMS"
