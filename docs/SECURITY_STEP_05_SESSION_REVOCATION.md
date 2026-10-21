# Step 05 — Legacy privileged session revocation

## Source change
In `src-tauri/src/auth.rs`, `session_get` no longer restores persisted `logs_admin` session markers. Instead, it calls `session_clear(db)?` and returns `SessionKind::None`.

This closes the stale-session startup path after the shared-credential login was disabled in Step 03.

## Boundaries
- Existing running processes may still hold a privileged in-memory session until restarted/logged out; this is not a remote session revocation mechanism.
- `session_set_logs_admin` remains defined and must be reviewed for call sites.
- Other legacy persisted sessions (owner/cashier/doctor) are unchanged.
- A replacement privileged account provisioning/login workflow is not implemented.
- No tests or build were run; validate DB startup, session migration and logout on Windows before release.

## Local acceptance tests
1. Set persisted session to `logs_admin`; restart: expect no authenticated session and cleared DB key.
2. Verify old shared-credential login is rejected.
3. Verify owner, cashier and doctor sessions retain documented behavior.
4. Verify doctor sessions for deleted doctors are invalidated.
5. Verify no sensitive credentials are logged.

No CI and no merge.
