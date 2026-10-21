# Step 06 — Authentication regression tests

Implemented in `src-tauri/src/auth.rs` under `#[cfg(test)] mod security_session_tests`:
- `persisted_legacy_logs_admin_is_revoked`
- `regular_role_login_remains_available`
- `owner_session_is_not_revoked_by_legacy_admin_cleanup`

These use `Db::new_in_memory()` and `setup_v2()` to avoid touching real clinic data.

## Run locally (not run by this change)
From repository root:
```powershell
cd src-tauri
cargo test security_session_tests
```

## Further hardening required
- Existing salted SHA-256 password hashes should be migrated to a password-specific KDF with a safe upgrade path.
- Add a new privileged account provisioning workflow before enabling logs-admin access again.
- Check every place that can call `session_set_logs_admin`.
- Verify no stale in-memory privileged session persists in an already-running process.
- Verify frontend typecheck/build against companion UI branch.
- Audit authorization for clinical read/write operations in specialty feature branches.

No tests/build/CI were executed and no merge was performed.
