# Synthetic plaintext export fixture

`plaintext.db` is frozen synthetic test data, not a user backup or production
secret. `portable_permissions.rs` copies it into a temporary installation to
verify that export workspaces and exported files have private permissions.
It does not test import or restore compatibility.

The database contains one synthetic USD account and one deposit of
`1234.567890123456789`, with a Unicode account name. The original source included
fake token, provider-secret, broker-reference and installation-identity values;
the upstream exporter sanitized them. The fixture is unencrypted and does not
require the source installation key.

`manifest.json` records its SHA-256 hash and producer app version, platform and
SQLCipher version. The bytes were authored upstream on macOS aarch64 with
app/storage 3.9.0, `libsqlite3-sys` 0.38.2 and SQLCipher 4.14.0 community at commit
`8eac3bfa77b11fa03459c232106a19f2e0c86f04`. Generation used random cryptographic
inputs and creation timestamps; it was not byte-reproducible. Keep these exact
bytes rather than regenerating them to make a failing test pass.

The protected import fixture and its authoring-only test were removed when
Wealthlog deleted import and user restore. Current protected export behavior is
covered by tests in `src/db/portable.rs`, not by reading a legacy protected file.

Run the retained permissions consumer from the repository root:

```sh
cargo test --locked -p wealthfolio-storage-sqlite --test portable_permissions
```
