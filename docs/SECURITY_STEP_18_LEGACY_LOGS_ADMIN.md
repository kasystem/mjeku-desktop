# Step 18 — Reject legacy logs-admin identity

## Finding
Despite disabling the standalone logs-admin password command and persisted logs-admin sessions in earlier steps, `auth_user_login` still contained a match arm capable of creating a logs-admin session if `user_verify_role` ever returned the legacy role. The shared `require_login` guard also exempted this role from normal license verification.

## Fix
- The `UserRole::LogsAdmin` match arm in `auth_user_login` now returns an error instead of writing a privileged session.
- `require_login` now rejects `SessionKind::User { role: LogsAdmin }` rather than granting an exemption.

## Scope
The normal verifier currently returns Owner or Cashier, so this is defensive hardening of a dormant path. Existing legacy enum variants and helper functions remain and should be removed after checking compatibility and tests.

## Validation
Not run. Verify with `cargo check` and `cargo test security_session_tests` from `src-tauri`. Review all references to `LogsAdmin` before removing its type variant.

No CI, build or merge was performed.
