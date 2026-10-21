# Step 17 — Revoke vendor-admin unlock on session switch

## Finding
Successful login could replace the active Owner/Cashier/Doctor identity without resetting the in-memory vendor-admin unlock flag. This risks privilege carryover to the next authenticated user.

## Fix
- `auth_user_login` calls `state.auth.admin_lock().await` before establishing a new successfully verified user session.
- `auth_doctor_login` calls `state.auth.admin_lock().await` after successful doctor credential verification and before establishing the new doctor session.
- Existing logout and invalid-session paths also clear admin unlock.

## Limits
- The unlock is still not time-bound; expiry/idle timeout and failed-login throttling remain outstanding.
- Session switching and admin unlock operations may race if multiple Tauri commands run concurrently. A single coordinated auth state transition would improve this.
- Only successful identity switches clear unlock in this change; unsuccessful login attempts do not change the active session.
- Tests and Windows build have not been executed.

## Local validation
```powershell
cd src-tauri
cargo check
cargo test security_session_tests
```

No CI or merge.
