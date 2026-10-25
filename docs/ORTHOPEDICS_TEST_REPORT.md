# Orthopedics lifecycle — verification notes (2026-10-25)

## Scope committed
SQLite migration v4 and Rust/Tauri endpoints implement authenticated close and
reopen operations, transactionally logged immutable status history, and
database-level prevention of closed-episode clinical edits. UI contains
a visible discharge/reopen panel and history; existing clinical pages show
a read-only warning.

## Automated cases added
`#[cfg(test)] mod orthopedic_lifecycle_tests` in `src-tauri/src/db.rs`
contains:
1. `discharge_requires_signed_final_summary_and_valid_transition_reason`:
   refusal without final summary, refusal of short reason, close with final
   report linked, refusal of duplicate close, refusal of short reopen.
2. `closed_episode_is_read_only_but_signed_amendments_and_reopening_work`:
   rejects marker writes and episode-upsert circumvention, rejects draft
   reports after close, permits reasoned signed amendment, rejects deletion
   of lifecycle event, reopens and permits new marker, preserves both events.

## Test execution status — verified on local Windows machine, 2026-10-25
The user supplied PowerShell output after pulling the fixes. On the actual Windows
checkout, `npm run typecheck` completed successfully, `npm run build` completed
successfully (Vite v5.4.21, 76 modules transformed, production UI artifacts),
and `npm run cargo:test` completed successfully: **50 passed; 0 failed**.
This includes both new orthopedic lifecycle regression tests.
Rust emitted 12 non-blocking compiler warnings. No workflow/CI was activated.
These results **do not** prove full Tauri/NSIS .exe packaging, hardware integration,
clinical E2E acceptance or medical certification.

### On Windows, in existing local checkouts
```powershell
cd "C:\Users\Admin\repos\Mjeku\mjeku-ui"
git switch feature/mjeku-pos-sef-integration-20261024
git pull --ff-only
npm ci
npm run typecheck
npm run build

cd "C:\Users\Admin\repos\Mjeku\mjeku-desktop"
git switch feature/sef-motor-fiskal
git pull --ff-only
npm install
npm run cargo:test
npm run tauri:dev
```

### Manual clinical acceptance
1. Add a real test patient and open two separate orthopedic episodes.
2. Try closing an episode without final summary: must be rejected.
3. Finalize a `final_summary` report while signed in as a doctor.
4. Close the episode with rationale: the history must show doctor/reason.
5. Attempt to add an injury marker, fracture review and surgery to closed
   episode: SQLite must reject each write.
6. Attempt closing it twice: must be rejected, no duplicate lifecycle event.
7. Add a signed report amendment with rationale: history versions preserved.
8. Reopen as a doctor with a reason; confirm subsequent writes are accepted
   and prior closure audit survives restart.
9. Attempt lifecycle action as owner or cashier: must be forbidden.
10. Restart Windows app, confirm episode status and transition history persist.

### Remaining verification
- TypeScript build and Rust unit tests: **passed on local Windows checkout** (see latest evidence below).
- Upgrade an existing SQLite clinic database containing legacy episodes.
- Windows/macOS packaging and platform E2E.
- Clinical sign-off, PACS, implant inventory and physiotherapy integration.

## Local Windows test evidence and repair (2026-10-25)

User-provided Windows PowerShell log after pulling the episode lifecycle commits:
- `mjeku-ui: npm run typecheck` failed with 2 TS2550 errors because the
  ES2020 target does not declare `String.prototype.replaceAll`.
- `mjeku-desktop: npm run cargo:test` compiled and ran **50 tests**;
  **46 passed, 4 failed**. Both newly added orthopedic lifecycle tests passed.
- The 4 failures were unrelated to clinical lifecycle logic:
  1. `generate_sample_invoice_pdf`: hardcoded developer desktop path missing.
  2. `generate_sample_visit_pdf`: same hardcoded developer path.
  3. `periodontal_exam_round_trip_preserves_nullable_sites`: test indexed
     results without accounting for SQLite's `ORDER BY tooth_fdi,site`.
  4. `orthopedics_schema_migration_is_idempotent`: expected 15 tables even
     though v4 introduced the 16th orthopedic table.

Code fixes have been committed directly to the existing development branches:
- UI: both display labels use ES2020-compatible `key.replace(/_/g, " ")`.
- PDF tests: validate in-memory PDF header, EOF marker and nontrivial length;
  do not write sample patient data to a developer-specific path.
- Periodontal test: identify MB/B measurements by tooth and site, assert
  nullable bleeding/plaque/suppuration and depth values for both sites.
- Orthopedic idempotence test: reruns v4 twice and checks 16 tables plus the
  lifecycle table and critical triggers.

**Post-fix verification status: PASSED in user-provided Windows PowerShell output.**
The 46/50 numbers above describe the *pre-fix* run. A subsequent Windows run
confirmed 50/50 Rust unit tests, TypeScript typechecking and Vite production UI build.
This does not replace clinical E2E verification or a Windows .exe packaging test.
The local generated `src-tauri/resources/ui-seed.zip` and
`ui-seed-version.txt` may be dirty; do not reset these user-local files
merely to run or pull the tests.

## Final post-fix Windows execution evidence — 2026-10-25

From the user's second PowerShell log after pulling `mjeku-ui` commit
`b0eeb1d` and `mjeku-desktop` commit `b2caf3a`:

| Check | Verified output |
| --- | --- |
| `npm run typecheck` | `tsc --noEmit` completed with no diagnostics |
| `npm run build` | Vite v5.4.21: 76 modules transformed, build successful; manifest and UI bundle written |
| `npm run cargo:test` | `test result: ok. 50 passed; 0 failed; 0 ignored` |
| Orthopedics lifecycle | Both new tests `...closed_episode_is_read_only...` and `...discharge_requires_signed_final_summary...` reported `ok` |
| Existing PDF and periodontal tests | All formerly failing tests reported `ok` |
| Rust warnings | 12, non-fatal, remain to be triaged |

Verified status is based on the user-provided log from their local Windows PC,
not a server-side test run. UI production build is not the same as installing
or launching the Tauri .exe. Do not claim patient-ready clinical certification.

## New, separate change: native PDF rendering and POS sidebar branding (2026-10-25)

After the 50/50 Windows baseline, additional code was committed:
- Original PROGRAMERI POS logo in sidebar and embedded PDF header.
- `orthopedic_report_pdf_data` verifies report/patient/episode via SQL joins.
- `orthopedic_report_export_pdf` builds an A4 multi-page PDF from a saved
  version, opens a native Windows save dialog, then launches the saved PDF
  in the system's PDF reader.
- `OrthopedicsPage.tsx` no longer uses `window.print()`; it presents a
  saved-version selector and calls the Tauri export command.
- New tests: one standalone PDF generation test and one DB+PDF data-binding
  test. These are **not part of the earlier 50/50 verified result**.

**New PDF feature status: NOT YET RUN ON WINDOWS.** Rerun full TypeScript
typecheck, Vite build and Cargo tests to confirm the changed application
compiles, then manually verify layout, Albanian glyphs, correct patient ID,
PDF selection, permission checks and printing from the PDF reader.
