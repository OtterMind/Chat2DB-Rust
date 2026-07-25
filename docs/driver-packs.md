# Managed JDBC Driver Packs

## Status

Implemented: hash-verified local driver-pack discovery, bounded startup preload,
and immutable driver inventory. Web and desktop hosts use the same Core path.

Not implemented: signature verification, remote catalogs, downloading,
installation, hot reload, automatic updates, compatibility selection, and
rollback. A local pack is executable code trusted at the same level as any
other user-installed JDBC driver.

## Location

The default root is `driver-packs/` below the Chat2DB data directory. Web and
desktop can override it with `CHAT2DB_DRIVER_PACK_DIR`. A missing root means no
managed drivers are installed. Each direct child directory is one pack:

```text
driver-packs/
  01-postgresql/
    driver-pack.json
    postgresql-42.7.7.jar
```

Regular files directly below the root are ignored, subject to the bounded root
entry count. Root entries that are symbolic links and pack directories that do
not contain `driver-pack.json` make startup fail closed.

`jdbc-driver-runtime/` below the data directory is an application-private,
owner-only staging and Java snapshot parent (`0700` on Unix; inherited
owner-only data-directory ACL on Windows). The storage lock makes it
single-owner. Startup rejects a symbolic-link replacement, removes stale state
left by a terminated generation, and recreates the directory before scanning.

## Manifest

`driver-pack.json` is strict JSON. Unknown fields are rejected.

```json
{
  "schemaVersion": 1,
  "id": "postgresql",
  "name": "PostgreSQL",
  "version": "42.7.7",
  "driverClass": "org.postgresql.Driver",
  "artifacts": [
    {
      "path": "postgresql-42.7.7.jar",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ]
}
```

Artifact order is significant because it contributes to the engine-derived
`driverId`. Paths use forward slashes, remain relative to their pack, and must
name regular JAR files. Absolute paths, `.` or `..`, Windows prefixes,
backslashes, colons, and symbolic-link components are rejected.

## Startup Semantics

Rust performs the following work before exposing the product runtime:

1. Open and exclusively lock product storage.
2. Reset the private JDBC runtime directory, then scan direct pack directories
   in deterministic path order with a bounded total entry count.
3. Parse each manifest and reject duplicate pack IDs.
4. Resolve each JAR inside its pack and open it without following the final
   symbolic link. Rust verifies that the opened handle is a bounded regular
   file and copies it into a private `host-*` staging directory.
5. Hash the staged bytes, compare the declared SHA-256, and reject duplicate
   driver identities derived from class plus ordered artifact digests.
6. Start one Java compatibility-engine generation with its own `engine-*`
   snapshot root.
7. Load packs sequentially. Java copies only the Rust-staged JARs into private
   per-driver snapshots with independent byte accounting and SHA-256
   verification before classloading them.
8. Publish the immutable inventory only after every pack loads successfully.
9. Delete Rust staging after preload. After the child process is fully reaped,
   Rust deletes the complete Java generation root, including snapshots Java
   could not remove while Windows file handles were open.

Any discovery or load error aborts startup. A load error shuts down the Java
generation, so callers never observe a partially loaded inventory.

## Limits

| Resource | Limit |
|---|---:|
| Manifest size | 64 KiB |
| Packs per root | 128 |
| All direct root entries | 256 |
| Artifacts per pack | 32 |
| One artifact | 256 MiB |
| All artifacts discovered at startup | 1 GiB |
| Relative artifact path | 1,024 UTF-8 bytes |

The bridge and Java snapshot path independently enforce the 256 MiB
per-artifact and 1 GiB aggregate limits. Java receives only private Rust
staging paths and rejects a digest mismatch before classloading.

## Inventory

Core returns `JdbcDriverList`. Axum exposes `GET /api/v1/drivers`; Tauri exposes
`list_drivers`. Each item contains `packId`, display name, pack version,
engine-derived `driverId`, `driverClass`, artifact count, and total artifact
bytes. Absolute paths and manifest digests are not returned.

Datasource records continue to store the derived `driverId`. A managed host
rejects create requests and driver changes whose ID is absent from the startup
inventory with `driver_not_installed`. An existing datasource may retain a
stale ID so it remains editable after a pack is removed. Non-managed
`RuntimeHost::from_supervisor` composition keeps accepting externally loaded
driver IDs.

Renaming the pack directory or changing display metadata does not change the
ID; changing the driver class, artifact order, or artifact bytes does.
