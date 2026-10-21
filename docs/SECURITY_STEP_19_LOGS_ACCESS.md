# Step 19 — Error logs authorization

## Finding
The `require_logs_admin` guard accepted only a legacy `LogsAdmin` session marker. Earlier security changes deliberately revoked persisted legacy sessions and disabled issuance of that role, leaving the error logs guard tied to an obsolete authorization model.

## Implemented
`require_logs_admin` now:
1. Requires an expiry-valid session via `require_login` (including license validation).
2. Requires the existing vendor-admin unlock state.
3. Denies access otherwise.

## Compatibility and risk
- Authorized users must log in and unlock admin before accessing logs. Verify frontend flow and any support workflows.
- The current policy does not require an Owner role: any authenticated user who knows the vendor-admin password can unlock. Confirm intended policy before production.
- Vendor-admin unlock has no independent expiry; rate limiting and authorization tests remain outstanding.
- Tests and builds were not run.

No CI and no merge.
