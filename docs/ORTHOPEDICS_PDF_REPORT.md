# Orthopedics — native A4 clinical PDF (2026-10-25)

## User-facing behavior

On PROGRAMERI MJEKU desktop, the sidebar's display branding is now
**PROGRAMERI POS** with the authentic POS emblem imported from the
`kasystem/pos-native-windows` source assets, as explicitly requested.
The underlying Tauri application identity and clinical database remain MJEKU.

The **Raportet** section now uses a dedicated, native A4 PDF document:
1. Sign in as a **Doctor** to create a saved clinical report version.
   The owner role alone cannot sign/finalize clinical documents.
2. Pick a saved report version from the dropdown in **Raportet**.
3. Click **Gjenero dhe ruaj PDF A4**, choose a destination in Windows.
4. The native Rust renderer obtains that exact immutable version and the
   associated patient from SQLite, produces PDF bytes, and writes the file.
5. Windows opens the saved file with its registered PDF reader; use that
   application's **Print** function to print the actual PDF.

No `window.print()`, no HTML/WebView screenshot and no client-provided
patient identity are involved in the PDF generation.

## Report layout

- Physical paper: ISO A4, 210 × 297 mm, with separate page breaks.
- Official teal PROGRAMERI POS logo embedded in the PDF binary.
- Institution display name (from the clinic setting), report title and type.
- Patient name and optional patient code from linked clinic patient record.
- Orthopedic diagnosis, region, laterality and episode ID.
- Report status (draft visibly marked), version, timestamp, amendment reason.
- Saved report narrative and the snapshot sections for fracture, ROM/special
  test, operation, implant and rehab data.
- Report author's authenticated doctor ID and immutable report ID.
- Subsequent pages receive their own header.

Clinical report data must come from a *persisted version*, not unsaved editor
fields. No PDF is generated until a report has been saved successfully.

## Implementation map

- `mjeku-ui/src/assets/programeri-pos-logo.png`: original POS brand asset
- `mjeku-ui/src/components/Sidebar.tsx`: branding
- `mjeku-ui/src/pages/OrthopedicsPage.tsx`: version picker, PDF button
- `mjeku-ui/src/api/client.ts`: `orthopedic_report_export_pdf` invocation
- `mjeku-desktop/src-tauri/src/db.rs`: SQL-joined patient/episode/report record
- `mjeku-desktop/src-tauri/src/orthopedics_pdf.rs`: native PDF renderer
- `mjeku-desktop/src-tauri/src/lib.rs`: authorized Tauri command and native file
  dialog, Windows PDF reader launch
- `mjeku-desktop/src-tauri/resources/programeri-pos-logo.png`: bundled brand logo

## Security and limits

- No anonymous/cashier report exports. Access is gated by the existing
  `require_orthopedic_clinician` authorization. Patient-level ACL is **not**
  implemented; it is a separate known clinical security gap.
- No overwrite of saved report versions occurs during PDF export.
- The document is not a legally signed digital PDF signature / PKI artifact.
  It includes the database-recorded author's ID and version metadata.
- The report snapshot may differ from later clinical records. The PDF uses the
  version actually selected, never a live reinterpretation of historical data.
- The user chooses the destination; exported patient PDFs must be handled
  under the clinic's confidentiality policies.
- The first rollout uses the built-in PDF fonts and must be visually reviewed
  for Albanian glyph rendering, large narratives, page breaking and margins.

## Testing required on Windows

```powershell
cd "C:\Users\Admin\repos\Mjeku\mjeku-ui"
git pull --ff-only
npm run typecheck
npm run build

cd "C:\Users\Admin\repos\Mjeku\mjeku-desktop"
git pull --ff-only
npm run cargo:test
# Stop old desktop dev process and restart it so the new Rust command registers.
npm run tauri:dev
```

If 5173 is occupied, use the previously tested separate Vite process at 5187
and pass `MJEKU_DEV_URL` with a Tauri dev config override to 5187.

Test an unsaved report (PDF disabled), a saved draft (DRAFT marker),
a doctor's signed final version, a previously amended version, a long report
that spans A4 pages, and confirm the saved PDF opens in a real PDF reader.
Verify that owner login cannot sign a new report. Preserve existing database
and resource files; no reset, no CI or merge.

**Status:** Code committed; new Rust renderer and UI binding have **not yet
been compiled or integration-tested on Windows** in this turn.
