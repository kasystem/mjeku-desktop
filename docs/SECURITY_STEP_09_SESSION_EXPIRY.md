# Step 09 — Persisted session expiry

## Implemented
- New setting `session_expires_at` stores an RFC3339 UTC expiry.
- `session_set_user`, `session_set_cashier` and `session_set_doctor` set an expiry 12 hours ahead.
- `session_get` rejects and clears expired, malformed-expiry and pre-upgrade sessions without expiry.
- `session_clear` clears the expiry setting.
- Added tests for expired owner session, pre-upgrade session without expiry, and new cashier session.

## Deployment and limitations
- **All pre-upgrade persisted sessions require reauthentication after updating.** Passwords/accounts are not deleted.
- Expiry is validated on `session_get`; a long-running in-memory session is not guaranteed to be rejected until state is revalidated. Command-level session checks need a separate audit.
- Wall-clock changes and concurrent session writes are not yet hardened. Session/expiry writes are separate SQLite setting operations, not one transaction.
- The legacy logs-admin role has no supported new login route.
- Password hashing remains salted SHA-256 and needs a backward-compatible Argon2id migration.

## Local validation (not executed)
```powershell
cd src-tauri
cargo test security_session_tests
```

No CI, merge or Windows build was run.
