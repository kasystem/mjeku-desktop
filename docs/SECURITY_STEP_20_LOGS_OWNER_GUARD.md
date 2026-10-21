# Step 20 — Owner-level diagnostic logs access

## Change
`require_logs_admin` now calls `require_owner` rather than `require_login`, and also requires the separate in-memory vendor-admin unlock.

`require_owner` allows Owner or an admin doctor and enforces the persisted session expiry and license checks through `require_login`.

## Reason
Error logs may include sensitive diagnostic or patient metadata. Knowing the vendor-admin password alone should not grant access to this resource for ordinary authenticated users.

## Compatibility
Cashiers and non-admin doctors can no longer access Error Logs even if they unlock the vendor-admin flag. Review the UI's access policy and support workflows.

## Outstanding
- Audit logs for sensitive data leakage and redaction.
- Implement admin unlock expiry and login attempt throttling.
- Validate concurrent session changes and UI integration.
- Tests and build have not been executed.

No CI or merge.
