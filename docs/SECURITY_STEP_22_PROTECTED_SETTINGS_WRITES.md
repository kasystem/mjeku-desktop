# Step 22 — Protected settings write authorization

## Finding
`settings_set` checked only the in-memory admin-unlocked flag for protected settings. If a session expired while that flag remained set, a protected write could bypass `require_login` and its license/session validation.

## Implemented
- Determine whether the application has been configured via `auth::is_configured`.
- For configured installations, protected settings now require `require_login` and `is_admin_unlocked`.
- Non-bootstrap protected keys also require these checks before initial configuration.
- Preserve the original first-time bootstrap allowance only for the five existing integration/update connection keys.
- Unprotected writes still require `require_login`.
- Generic settings writes to authentication/session keys remain blocked by the earlier Step 12 change.

## Remaining risks
- First-time bootstrap is still accessible before setup for explicitly allowed integration keys; evaluate a dedicated one-time provisioning flow.
- No additional owner-role requirement was added beyond the existing vendor-admin unlock policy.
- The in-memory admin unlock does not yet have its own expiry.
- Local tests and Windows build have not been run; check first-run setup and existing-settings UI behavior.

No CI and no merge.
