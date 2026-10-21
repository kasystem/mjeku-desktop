# Step 11 — Session expiry at shared command guards

## Implemented
- `require_login` calls `auth::session_get` from a blocking task to validate persisted session state and expiry before granting access.
- On invalid sessions, cached auth state is cleared and administrative unlock is locked.
- `require_owner` and `require_finance` inherit this check through `require_login`.
- `require_logs_admin` now reads persisted state rather than trusting a cached in-memory role.

## Important limitations
- Only commands that actually call these guards are covered. An audit of the entire `tauri::generate_handler!` command list and all sensitive commands is still required.
- Session expiry and authorization checks are not atomic with downstream data mutations; concurrent logout and operations need testing.
- Doctor-level and patient-level data access checks require dedicated audit.
- Password hashing still needs a backward-compatible memory-hard KDF migration.
- Tests and Windows build have **not** been run.

## Local checks
```powershell
cd src-tauri
cargo test security_session_tests
cargo check
```

No CI and no merge.
