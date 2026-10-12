# Vendored sqlx-sqlite fork

`vendor/sqlx-sqlite/` is a copy of the `sqlx-sqlite` 0.9.0 source from
crates.io. Its manifest sets `libsqlite3-sys` to `0.38.0`, so the workspace
unifies on a single native SQLite bindings version
(rusqlite 0.40 / crabapple 0.4.7 use 0.38). Cargo's `links` rule permits only
one libsqlite3-sys per dependency graph. Released `sqlx-sqlite` 0.9.0 asks for
`>=0.30.1, <0.38.0`, so without this change sqlx 0.9 and rusqlite 0.40 cannot
coexist.

sqlx's `main` branch already allows `<0.39.0`. The fork can go once a sqlx
release carries that range.

## Re-vendoring on sqlx upgrades

1. Note the new `sqlx-sqlite` version from the lockfile after the sqlx
   bump, then download that version's `.crate` tarball from crates.io into
   a new empty directory, and check its SHA-256 against the `checksum` that
   `https://crates.io/api/v1/crates/sqlx-sqlite/<version>` reports.
2. Copy its `src/`, `Cargo.toml`, `Cargo.toml.orig` and
   `.cargo_vcs_info.json` over the ones in `vendor/sqlx-sqlite/`, then
   re-apply the manifest changes to `Cargo.toml`: set `libsqlite3-sys` to
   the rusqlite-matched release (`0.38.0` unless rusqlite moved), keep
   `sqlx-core` pinned to the matching version (`=0.9.0`), keep the
   inherited keys (version, license, edition) written out as the published
   manifest already has them, and drop the `[lints.clippy]` block, which
   names upstream's workspace lints.
3. Leave `LICENSE-MIT` and `LICENSE-APACHE` as they are, because the 0.9.0
   tarball carries only the paths `../LICENSE-MIT` and `../LICENSE-APACHE`
   in those files, not the license texts. Leave out the tarball's
   `Cargo.lock`, because Cargo ignores it for a path dependency and GitHub
   would scan it as a second lockfile.
4. Run the full verification suite: the server's SQLite tests are the gate
   for this combination, because upstream tests sqlx-sqlite only against its
   own libsqlite3-sys range. `cargo tree -i libsqlite3-sys` must show one
   version.
5. Never edit fork source beyond the manifest. Upstream license:
   MIT OR Apache-2.0 (both license files stay in the vendor dir).
