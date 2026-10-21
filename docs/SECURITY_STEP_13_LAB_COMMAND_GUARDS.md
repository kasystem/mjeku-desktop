# Step 13 — Laboratory command access guards

## Finding
Seven laboratory Tauri commands accessed laboratory devices, local configuration, and the inbox without the shared login guard.

## Changes
Added `require_login(&state).await?` to:
- `lab_device_get_default`
- `lab_device_set_default`
- `lab_device_connect`
- `lab_device_disconnect`
- `lab_device_status`
- `lab_inbox_list`
- `lab_inbox_assign`

These commands now check the shared session guard before performing their existing operations. `require_login` revalidates the persisted session and expiry.

## Limitations
- This is authentication, **not** laboratory role-based access control or patient-specific authorization. A dedicated lab role/permission matrix is still needed.
- Remaining Tauri commands require inspection for missing guards.
- Serial device access outside these Tauri commands has not been audited.
- Tests and build have not been run.

## Local verification
```powershell
cd src-tauri
cargo check
cargo test security_session_tests
```

No CI and no merge.
