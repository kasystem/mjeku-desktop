# PROGRAMERI MJEKU / POS integration status — 2026-10-28

This records the boundary of the initial catalog and SEF bridge implementation. It is not a production certification or release approval.

## Implemented in source

- Mjeku services and products have editable ATK tax, category and unit metadata, SKU/barcode, category/subcategory, stock kind, purchase/sale price, purchase unit and conversion multiplier.
- The catalog values come from the POS ATK code list. Tax codes A, C, D and E retain their separate meanings and fixed rates; UI and Rust validate the same rules.
- Legacy local SQLite rows are preserved through additive, repeatable columns. Existing products default to `unknown` stock kind and blank ATK category/unit so they require review before fiscal use.
- Product intake converts purchase units to base units before creating the local stock movement. Product sales and edits run in an immediate SQLite transaction; retries are idempotent, insufficient stock is rejected, and fiscalized sales/items cannot be edited or deleted through these commands.
- Visit lines linked to Mjeku services carry stable Mjeku catalog IDs and ATK metadata over the existing SEF fiscalization bridge. Existing callers without metadata keep the old v1 hash. POS validates the metadata and rejects same-receipt grouped rows whose tax/catalog codes conflict.

## Data and deployment boundary

- `src-tauri/migrations/product_catalog_atk_v1.sql` is an additive Supabase migration draft. It has **not** been applied. The new stock tables need clinic-scoped RLS/grants reviewed before enabling remote sync; this change deliberately does not create broad anonymous policies. Existing rows in newly added columns are not assigned a clinic or guessed classification.
- The desktop repository's `resources/ui-seed.zip` and `ui-seed-version.txt` already had unrelated local edits. They were preserved. Therefore these UI sources have not been rebuilt into or verified through the embedded UI seed/update package.
- POS fiscalization is not an inventory API. No Mjeku stock movement is sent through fiscalization and there is no stable Mjeku-to-POS product association or authorized POS inventory operation in the existing bridge. The Mjeku stock ledger remains Mjeku-owned until a separate authenticated inventory contract is designed and implemented.

## Explicit gaps

- **Business identity/read-only settings:** POS launch currently passes only the named-pipe/token/context plus a diagnostic launch-mode flag. POS bridge status exposes trade/legal display name, branch name and POS number, but not NUI or address. Mjeku settings still allow editing its clinic identity fields, and `settings_set` does not enforce linked-mode identity protections. Do not treat the diagnostic environment variable as authorization. POS must expose an authoritative identity payload first; then protect those keys in both UI and backend and test direct command calls.
- **Onboarding:** completed onboarding is not newly locked by this change; it needs the same linked-mode identity source and backend authorization boundary.
- **SEF platform bridge:** current Mjeku transport is a Windows named pipe. The Mjeku Android bound-service path remains TODO; macOS/iOS transport is not implemented. No cross-platform device tests were run. POS mobile/macOS/iOS counterparts have not been verified for this metadata extension.
- **Printer selection:** existing mechanisms were not changed or tested. No claim is made for Windows installed printers, USB/RAW, network, Bluetooth, Android native print or iOS AirPrint.
- **Operational integration:** no live POS-to-Mjeku session, real SEF TEST coupon, physical printer, cloud migration, offline reconnect, upgrade from a copied customer-shaped database, or device test was performed. No production fiscal coupon was issued.
- **Warehouse and POS association:** this iteration does not introduce warehouse-level selection or a durable POS product mapping. Product stock remains in Mjeku's existing clinic ledger.

## Verification performed

- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, 65 tests, including additive migration preservation/idempotency and concurrent stock-out coverage.
- `npm run typecheck` in `mjeku-ui`: passed.
- `dotnet test tests/Programeri.Pos.Tests/Programeri.Pos.Tests.csproj --filter FullyQualifiedName~MjekuBridgeTests`: 40 passed.
- These are code-level tests only. No distribution build, installer, CI, release, migration application, or push was run.
