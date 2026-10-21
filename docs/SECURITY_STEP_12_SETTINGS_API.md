# Step 12 — Protect authentication settings from generic writes

## Finding
The generic Tauri `settings_set` endpoint did not classify the new `session_expires_at` setting as protected. It could therefore be modified by an authenticated caller through the generic settings API, undermining persisted session expiry.

## Fix
- `settings_set` rejects all writes to `session_expires_at`, `session`, `user_logged_in`, and admin/owner/cashier password salt/hash keys, including with vendor admin unlock.
- Authentication code paths continue to use `Db::setting_set` internally to manage their own state.
- `settings_get_all` hides `session_expires_at` from non-admin-unlocked users.

## Scope
This fixes the generic settings API, not direct database modification or any other command that might expose `Db::setting_set`. Further audit required of all Tauri commands, secrets returned to admin-unlocked users, and provisioning flows. Verify that no legitimate UI workflow depends on generic writes to these keys.

## Validation
Not run. From `src-tauri`:
```powershell
cargo test security_session_tests
cargo check
```

No merge or CI.
