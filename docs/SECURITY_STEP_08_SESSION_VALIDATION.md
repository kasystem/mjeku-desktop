# Step 08 — Invalid persisted session cleanup

## Implemented
- `session_get` clears a persisted `doctor:` marker with an empty doctor identifier.
- `session_get` clears unrecognized session marker strings rather than retaining corrupted or stale values.
- Added two unit tests: `malformed_doctor_session_is_cleared` and `unknown_session_marker_is_cleared`.
- Seven regression tests now exist in `security_session_tests` (not executed in this environment).

## Scope and limitations
This is **not** session expiration. Explicit persisted `owner`, `cashier` and valid doctor sessions still survive restart without an expiry timestamp. A complete expiry/revocation design needs versioned session records, monotonic validation of time where possible, and explicit sign-out on credential changes.

Password storage still uses salted SHA-256; migrate safely to a memory-hard password KDF before claiming production-ready security. Clinical patient authorization has not been exhaustively audited.

## Local validation
```powershell
cd src-tauri
cargo test security_session_tests
```
Do not merge or deploy until Rust tests, UI integration, role authorization and Windows build are verified.

No CI, merge or build performed.
