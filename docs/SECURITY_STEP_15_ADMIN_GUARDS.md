# Step 15 — Privileged command guards

## Finding
`auth_admin_change_password` did not check an active authenticated session or admin unlock before changing the vendor-admin password. `admin_reset_clinic` checked admin unlock but not whether the login session was still valid.

## Implemented
- `auth_admin_change_password` now requires both `require_login` and `state.auth.is_admin_unlocked()`.
- `admin_reset_clinic` now requires `require_login` before its existing admin-unlock check.
- Both inherit persisted session-expiry verification via `require_login`.

## Compatibility
- Changing the vendor-admin password now requires a valid login and prior admin unlock. Review the frontend settings flow to ensure it calls `auth_admin_unlock` first.
- Resetting the clinic requires a valid login as well as admin unlock.
- This change does not require a specific Owner role beyond the existing admin unlock; the role policy needs product-level review.

## Remaining risks
- The admin unlock is an in-memory flag; review its lifetime and relationship to session changes.
- Audit other commands for missing checks and verify UI behavior.
- Tests and Windows build have not been executed.

No CI and no merge.
