#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
desktop_root="${repository_root}/apps/chat2db-desktop"
build_target="${CHAT2DB_MACOS_BUILD_TARGET:-${repository_root}/target/macos-package-build}"
app_path="${build_target}/release/bundle/macos/Chat2DB Rust.app"
package_directory="${repository_root}/target/macos-package"
staging_directory=""
notary_directory=""
verification_directory=""
dmg_mounted=false

cleanup() {
  if [[ "${dmg_mounted}" == true && -n "${verification_directory}" ]]; then
    hdiutil detach "${verification_directory}" >/dev/null 2>&1 || true
    dmg_mounted=false
  fi
  if [[ -n "${staging_directory}" && -d "${staging_directory}" ]]; then
    rm -rf -- "${staging_directory}"
  fi
  if [[ -n "${notary_directory}" && -d "${notary_directory}" ]]; then
    rm -rf -- "${notary_directory}"
  fi
  if [[ -n "${verification_directory}" && -d "${verification_directory}" ]]; then
    rm -rf -- "${verification_directory}"
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
signing_keychain="${CHAT2DB_SIGNING_KEYCHAIN:-}"
notary_profile="${CHAT2DB_NOTARY_KEYCHAIN_PROFILE:-}"
expected_team_id="${APPLE_TEAM_ID:-}"
notarization_enabled=false
notarization_status="not-submitted"
distribution_status="internal-test-only"

if [[ -n "${notary_profile}" ]]; then
  if [[ "${signing_identity}" == "-" ]]; then
    echo "notarization requires a Developer ID signing identity" >&2
    exit 1
  fi
  if [[ -z "${signing_keychain}" || "${signing_keychain}" != /* || ! -f "${signing_keychain}" || -L "${signing_keychain}" ]]; then
    echo "notarization requires a safe signing keychain" >&2
    exit 1
  fi
  if [[ -z "${expected_team_id}" ]]; then
    echo "notarization requires APPLE_TEAM_ID" >&2
    exit 1
  fi
  notarization_enabled=true
fi

if [[ "${signing_identity}" == "-" ]]; then
  codesign --force --deep --sign - --timestamp=none "${app_path}"
  "${repository_root}/scripts/verify-macos-package.sh" "${app_path}"
else
  # Tauri owns the only Developer ID signing pass so nested runtime
  # entitlements and signatures are not destroyed by a deep re-sign.
  CHAT2DB_REQUIRE_DEVELOPER_ID_SIGNATURE=true \
    APPLE_TEAM_ID="${expected_team_id}" \
    "${repository_root}/scripts/verify-macos-package.sh" "${app_path}"
  distribution_status="developer-id-signed"
fi

notarize_artifact() {
  local artifact_path="$1"
  xcrun notarytool submit "${artifact_path}" \
    --keychain-profile "${notary_profile}" \
    --keychain "${signing_keychain}" \
    --wait \
    --timeout 45m
}

verify_developer_id_signature() {
  local artifact_path="$1"
  local artifact_kind="$2"
  local signature_details
  local signing_team_id
  local signing_authority
  local signing_timestamp
  local designated_requirement

  codesign --verify --strict --verbose=2 "${artifact_path}"
  signature_details="$(codesign -dv --verbose=4 "${artifact_path}" 2>&1)"
  signing_team_id="$(awk -F= '/^TeamIdentifier=/ { print $2; exit }' <<<"${signature_details}")"
  signing_authority="$(awk -F= '/^Authority=/ { print $2; exit }' <<<"${signature_details}")"
  signing_timestamp="$(awk -F= '/^Timestamp=/ { print $2; exit }' <<<"${signature_details}")"
  designated_requirement="$(codesign -d -r- "${artifact_path}" 2>&1)"

  if [[ "${signing_authority}" != Developer\ ID\ Application:* ]]; then
    echo "${artifact_kind} is not signed by a Developer ID Application identity" >&2
    exit 1
  fi
  if [[ -z "${signing_team_id}" || "${signing_team_id}" == "not set" ]]; then
    echo "${artifact_kind} is missing a TeamIdentifier" >&2
    exit 1
  fi
  if [[ -n "${expected_team_id}" && "${signing_team_id}" != "${expected_team_id}" ]]; then
    echo "${artifact_kind} TeamIdentifier does not match APPLE_TEAM_ID" >&2
    exit 1
  fi
  if [[ -z "${signing_timestamp}" || "${signing_timestamp}" == "none" ]]; then
    echo "${artifact_kind} is missing a trusted timestamp" >&2
    exit 1
  fi
  if [[ "${designated_requirement}" == *"cdhash"* ]]; then
    echo "${artifact_kind} still has a build-specific cdhash requirement" >&2
    exit 1
  fi
}

verify_packaged_app() {
  local packaged_app="$1"
  if [[ "${signing_identity}" == "-" ]]; then
    "${repository_root}/scripts/verify-macos-package.sh" "${packaged_app}"
  else
    CHAT2DB_REQUIRE_DEVELOPER_ID_SIGNATURE=true \
      APPLE_TEAM_ID="${expected_team_id}" \
      "${repository_root}/scripts/verify-macos-package.sh" "${packaged_app}"
  fi
  if [[ "${notarization_enabled}" == true ]]; then
    xcrun stapler validate "${packaged_app}"
    spctl --assess --type execute --verbose=4 "${packaged_app}"
  fi
}

if [[ "${notarization_enabled}" == true ]]; then
  notary_directory="$(mktemp -d "${target_root}/.chat2db-notary.XXXXXX")"
  notary_app_zip="${notary_directory}/Chat2DB-Rust.app.zip"
  ditto -c -k --sequesterRsrc --keepParent "${app_path}" "${notary_app_zip}"
  notarize_artifact "${notary_app_zip}"
  xcrun stapler staple "${app_path}"
  xcrun stapler validate "${app_path}"
  spctl --assess --type execute --verbose=4 "${app_path}"
  rm -rf -- "${notary_directory}"
  notary_directory=""
  CHAT2DB_REQUIRE_DEVELOPER_ID_SIGNATURE=true \
    APPLE_TEAM_ID="${expected_team_id}" \
    "${repository_root}/scripts/verify-macos-package.sh" "${app_path}"
  notarization_status="accepted"
  distribution_status="developer-id-notarized"
fi

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

verification_directory="$(mktemp -d "${target_root}/.chat2db-zip-verify.XXXXXX")"
ditto -x -k "${zip_path}" "${verification_directory}"
verify_packaged_app "${verification_directory}/Chat2DB Rust.app"
rm -rf -- "${verification_directory}"
verification_directory=""

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

if [[ "${signing_identity}" != "-" ]]; then
  if [[ -n "${signing_keychain}" ]]; then
    codesign --force --sign "${signing_identity}" --keychain "${signing_keychain}" --timestamp "${dmg_path}"
  else
    codesign --force --sign "${signing_identity}" --timestamp "${dmg_path}"
  fi
  verify_developer_id_signature "${dmg_path}" "macOS DMG"
fi

if [[ "${notarization_enabled}" == true ]]; then
  notarize_artifact "${dmg_path}"
  xcrun stapler staple "${dmg_path}"
  xcrun stapler validate "${dmg_path}"
  spctl --assess --type open --context context:primary-signature --verbose=4 "${dmg_path}"
fi
hdiutil verify "${dmg_path}"

verification_directory="$(mktemp -d "${target_root}/.chat2db-dmg-verify.XXXXXX")"
hdiutil attach -readonly -nobrowse -mountpoint "${verification_directory}" "${dmg_path}"
dmg_mounted=true
verify_packaged_app "${verification_directory}/Chat2DB Rust.app"
hdiutil detach "${verification_directory}"
dmg_mounted=false
rm -rf -- "${verification_directory}"
verification_directory=""

(
  cd "${package_directory}"
  shasum -a 256 -- "$(basename "${zip_path}")" "$(basename "${dmg_path}")" > SHA256SUMS
)
git_commit="$(git -C "${repository_root}" rev-parse HEAD)"
community_commit="$(git -C "${repository_root}/third_party/chat2db-community" rev-parse HEAD)"
app_kib="$(du -sk "${app_path}" | awk '{ print $1 }')"
signature_details="$(codesign -dv --verbose=4 "${app_path}" 2>&1)"
signing_team_id="$(awk -F= '/^TeamIdentifier=/ { print $2; exit }' <<<"${signature_details}")"
signing_authority="$(awk -F= '/^Authority=/ { print $2; exit }' <<<"${signature_details}")"
signing_team_id="${signing_team_id:-none}"
signing_authority="${signing_authority:-adhoc}"
cat > "${package_directory}/BUILD-MANIFEST.txt" <<EOF
Chat2DB Rust macOS test package
version=${version}
architecture=${artifact_arch}
git_commit=${git_commit}
community_commit=${community_commit}
app_size_kib=${app_kib}
signing_identity=${signing_identity}
signing_authority=${signing_authority}
signing_team_id=${signing_team_id}
notarization_status=${notarization_status}
distribution_status=${distribution_status}
EOF

echo "Built self-contained macOS app: ${app_path}"
echo "Packaged ZIP: ${zip_path}"
echo "Packaged DMG: ${dmg_path}"
echo "Checksums: ${package_directory}/SHA256SUMS"
