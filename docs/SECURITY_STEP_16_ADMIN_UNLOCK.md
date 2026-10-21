# Step 16 — Session-bound admin unlock

## Finding
`auth_admin_unlock` verified the admin password without first requiring an active, expiry-valid user session. A failed unlock attempt also did not clear a previously unlocked in-memory admin state.

## Fix
- Call `require_login(&state).await?` before admin-password verification.
- Call `state.auth.admin_lock().await` before checking the supplied admin password.
- Unlock only on successful `admin_verify`.

## Scope and limitations
- This requires an authenticated session for vendor-admin unlock and prevents stale unlock from surviving a failed retry.
- This does **not** add rate limiting, lockout, admin-unlock TTL, or per-operation re-verification.
- The frontend must not depend on admin unlock before ordinary login.
- Local tests and build have not been executed.

## Suggested local verification
```powershell
cd src-tauri
cargo check
cargo test security_session_tests
```

No CI and no merge.
