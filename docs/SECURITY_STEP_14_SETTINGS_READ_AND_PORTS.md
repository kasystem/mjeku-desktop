# Step 14 — Settings read and serial port access

## Implemented
- `settings_get_all` now requires `require_login` before reading clinic settings from SQLite.
- `lab_serial_ports_list` now accepts Tauri `AppState` and requires `require_login` before enumerating serial ports.
- Both reuse the expiry-aware shared authentication guard.

## Compatibility
- The frontend must be authenticated before loading clinic settings or listing serial ports.
- The `lab_serial_ports_list` Tauri command name is unchanged; the added `State` argument is injected by Tauri, so callers need not pass it.

## Outstanding
- Audit all other commands without shared guards, especially administrative operations, update/restart operations, and any clinical commands defined in other modules.
- Role and patient-scoped authorization are not implemented by these checks.
- Local Rust tests, cargo check and UI build have not been run.

No CI and no merge.
