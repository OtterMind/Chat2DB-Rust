# Third-Party Notices

This local test package contains the exact original Chat2DB Community frontend
and fixed Java compatibility classpath pinned by this repository. Their source
revision, artifact names, byte lengths, and SHA-256 digests are recorded in:

- `scripts/community-frontend.lock.json`
- `third_party/community-h2-classpath.lock`
- `target/macos-driver-packs/01-mysql/driver-pack.json`
- `target/macos-driver-packs/02-h2-migration/driver-pack.json`

The H2 2.1.214 driver is bundled only for read-only migration of the previous
Chat2DB local store. H2 is available under MPL 2.0 or EPL 1.0.

The public package driver-pack allowlist contains only MySQL Connector/J and
the H2 migration driver. It does not contain the proprietary DM JDBC driver.

The package includes copies of both the Chat2DB Rust license and the pinned
Chat2DB Community license under `Contents/Resources/chat2db/licenses`.

This notice is an inventory aid for an internal test build. It is not the
complete NOTICE/SBOM, commercial authorization, Developer ID signature, or
notarization evidence required for an external Object-form release.
