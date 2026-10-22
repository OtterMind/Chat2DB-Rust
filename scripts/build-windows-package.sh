#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
desktop_root="${repository_root}/apps/chat2db-desktop"
node "${repository_root}/scripts/product-version.mjs" --check
version="$(node "${repository_root}/scripts/product-version.mjs")"
build_target="${CHAT2DB_WINDOWS_BUILD_TARGET:-${repository_root}/target/windows-package-build}"
package_directory="${repository_root}/target/windows-package"
license_resource_directory="${repository_root}/target/windows-license-resources"

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

rm -rf -- "${license_resource_directory}"
mkdir -p -- "${license_resource_directory}"
cp -- "${repository_root}/LICENSE" \
  "${license_resource_directory}/Chat2DB-Rust-LICENSE.txt"
cp -- "${repository_root}/third_party/chat2db-community/LICENSE" \
  "${license_resource_directory}/Chat2DB-Community-LICENSE.txt"

for path in \
  "${repository_root}/target/windows-runtime/bin/java.exe" \
  "${repository_root}/java/compat-runtime/target/chat2db-compat-runtime-0.1.0-SNAPSHOT.jar" \
  "${repository_root}/target/community-h2-classpath" \
  "${repository_root}/target/windows-driver-packs" \
  "${license_resource_directory}/Chat2DB-Rust-LICENSE.txt" \
  "${license_resource_directory}/Chat2DB-Community-LICENSE.txt" \
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
cli_build_target="${repository_root}/target/windows-cli-build"
cli_resource_directory="${repository_root}/target/windows-cli"
rm -rf -- "${cli_build_target}"
rm -rf -- "${cli_resource_directory}"
mkdir -p -- "${cli_resource_directory}"
CARGO_TARGET_DIR="${cli_build_target}" RUSTUP_TOOLCHAIN="${rust_toolchain}" \
  cargo build -p chat2db-cli --release --locked
cp -- "${cli_build_target}/release/chat2db.exe" "${cli_resource_directory}/chat2db.exe"
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

signing_variables=(WIN_SERVER_IP WIN_SERVER_USER WIN_SSH_PRIVATE_KEY REMOTE_SIGN_PATH REMOTE_SIGN_SCRIPT HOST_KEY)
configured_signing_variables=0
for variable in "${signing_variables[@]}"; do
  if [[ -n "$(printenv "${variable}" || true)" ]]; then
    configured_signing_variables=$((configured_signing_variables + 1))
  fi
done
code_signing=skipped
if ((configured_signing_variables > 0)); then
  if ((configured_signing_variables < ${#signing_variables[@]})); then
    echo "Windows code signing requires all of: ${signing_variables[*]}" >&2
    exit 1
  fi
  # Sign the installers before the checksums and manifest describe the shipped bytes.
  # MSYS rewrites environment values that look like Unix paths when it launches a
  # native program, which would corrupt the signer's remote paths and script name.
  conversion_exclusions="$(IFS=';'; echo "${signing_variables[*]}")"
  for artifact in "${package_directory}"/*.exe "${package_directory}"/*.msi; do
    MSYS2_ENV_CONV_EXCL="${conversion_exclusions}" \
      pwsh -NoProfile -NonInteractive -File "$(cygpath -w "${repository_root}/scripts/sign-windows-package.ps1")" \
        -PackagePath "$(cygpath -w "${artifact}")"
  done
  code_signing=authenticode-dual
elif [[ "${CHAT2DB_REQUIRE_WINDOWS_SIGNING:-false}" == "true" ]]; then
  echo "Windows code signing is required but not configured: ${signing_variables[*]}" >&2
  exit 1
else
  echo "Windows code signing skipped: the remote signing service is not configured" >&2
fi

(
  cd "${package_directory}"
  sha256sum ./*.exe ./*.msi > SHA256SUMS
  {
    echo "Chat2DB Rust Windows package"
    echo "version=${version}"
    echo "git_commit=$(git -C "${repository_root}" rev-parse HEAD)"
    echo "community_commit=$(git -C "${repository_root}/third_party/chat2db-community" rev-parse HEAD)"
    echo "architecture=x86_64"
    echo "target=windows"
    echo "rust_toolchain=${rust_toolchain}"
    echo "tauri_cli=$(cargo tauri --version)"
    echo "code_signing=${code_signing}"
    echo "nsis=$(basename "${nsis_artifacts[0]}")"
    echo "msi=$(basename "${msi_artifacts[0]}")"
  } > BUILD-MANIFEST.txt
)

cli_zip_name="chat2db-cli-windows-x86_64.zip"
pwsh -NoProfile -NonInteractive -Command \
  "Compress-Archive -LiteralPath '$(cygpath -w "${cli_resource_directory}/chat2db.exe")' -DestinationPath '$(cygpath -w "${package_directory}/${cli_zip_name}")' -Force"
(
  cd "${package_directory}"
  sha256sum -- "${cli_zip_name}" > "${cli_zip_name}.sha256"
)

echo "Built Windows installers at ${package_directory}"
echo "Checksums: ${package_directory}/SHA256SUMS"
