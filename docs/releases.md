# Desktop releases

The product version comes from `[workspace.package].version` in `Cargo.toml`.
Rust binaries, CLI status, MCP server information, and frontend build/dev
configuration use that version. Tauri and npm metadata are checked-in
projections, verified before packaging and by CI.

## Prepare a version

1. Change `Cargo.toml` to the intended product version.
2. Run `rtk proxy node scripts/product-version.mjs --sync`. This updates Tauri,
   npm metadata, and only the local workspace packages in `Cargo.lock`.
3. Run `rtk make generate-contracts` to regenerate OpenAPI and TypeScript.
4. Run `rtk make verify`, `rtk make check-contracts`, and `rtk git diff --check`.
5. Review and merge the release preparation into `main`.

The Java compatibility engine and internal protocol have independent versions.
Product version synchronization does not rewrite Java artifacts, dependency
versions, or the pinned Community source.

## Build a release draft

Create an annotated tag named exactly `v<product version>` on the release
commit in `main`. The tag annotation records:

- `source_repository`: `OtterMind/Chat2DB-Rust`;
- `source_commit`: the complete release commit SHA;
- `community_ref`: the pinned submodule commit;
- `package_repository`: `OtterMind/Chat2DB-Rust`;
- `workflow_ref`: the complete release commit SHA;
- `product_workflow`: `.github/workflows/package.yml`; and
- `actions_parameters`: `none` (the workflow has no dispatch inputs).

Read the annotation with `rtk git show --no-patch v0.0.1`, then push the tag
with `rtk git push origin v0.0.1`. The `Desktop Packages` workflow will:

1. Check the product version, exact tag/version match, annotated tag type,
   and that the release commit is in `main`.
2. Run the reusable repository CI on that same tagged revision.
3. Build macOS ARM64/x86_64, Windows x86_64, and Linux ARM64/x86_64 packages.
   macOS signing and notarization remain required.
4. Require successful CI and every package job before preparing the release.
5. Verify package hashes and all five version/source/Community manifests.
6. Create a GitHub **Draft Release** with twelve package files, a combined
   `SHA256SUMS`, and five uniquely named `BUILD-MANIFEST-<platform>.txt` files.

Main-branch pushes and branch dispatches still produce Actions artifacts.
They do not create a Release. A tag can also be rebuilt with:

```bash
rtk gh workflow run package.yml --repo OtterMind/Chat2DB-Rust --ref v0.0.1
```

A rerun can replace assets on an existing draft. It refuses to replace assets
on a published release. Partial or failed builds do not reach the release job.

## Publish

Download the draft assets, verify `SHA256SUMS`, and validate installation,
startup, the displayed version, database connections, and CLI attachment on
the supported platforms. Review the draft notes and known limitations, and
complete the repository's Community distribution, NOTICE/SBOM, and installed
package acceptance requirements before publishing the draft.

Publish through GitHub or:

```bash
rtk gh release edit v0.0.1 --repo OtterMind/Chat2DB-Rust --draft=false
```
