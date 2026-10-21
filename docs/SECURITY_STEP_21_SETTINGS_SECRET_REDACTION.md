# Step 21 — Always redact authentication secrets

## Implemented
`settings_get_all` now unconditionally removes the following authentication and session values before returning its map to the frontend:
- `admin_salt`, `admin_hash`
- `user_salt`, `user_hash`
- `cashier_salt`, `cashier_hash`
- `user_logged_in`, `session`, `session_expires_at`

This applies regardless of vendor-admin unlock state. Existing admin-only visibility rules for integration settings (`supabase_url`, `supabase_api_key`, `supabase_anon_key`, `update_base_url`, `desktop_update_api`) are preserved.

## Why
Password hashes and session markers should not be exposed by a generic settings read API. Credential management uses dedicated authentication operations.

## Remaining verification
- Search the frontend for consumers that assume the generic settings map contains authentication metadata.
- Add unit/integration tests for both admin-unlocked and regular sessions.
- Review all other settings keys for additional sensitive values.
- Run `cargo check`, `cargo test` and a Windows smoke test locally.

Changes committed on `feature/security-hardening-step03`. No merge, CI or build executed.
