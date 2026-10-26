# Fizioterapi & Rehabilitim — Implementation status (2026-10-26)

## Source and scope
Implements the current `Fizioterapi.docx` and the shared rehabilitation
requirements in `Ortopedi.docx`, using existing React, Tauri, Rust and SQLite.
No GitHub Actions, CI changes or merges were performed.

## Existing implementation extended
- `physiotherapy_episodes` links to existing orthopedic episodes, patient IDs
  and orthopedic rehab plans. Clinical identity is checked via SQL joins.
- `physiotherapy_sessions` records author/date/intervention, response, NRS pain
  before/after, optional ROM/MMT and follow-up plan in an append-only log.
- Orthopedics' rehabilitation tab creates a linked referral and displays
  sessions and clinical progress reports from physiotherapy.
- Sidebar and department routing expose the new dedicated Physiotherapy page.

## New clinical core: physiotherapy_v2.sql
1. Structured `physiotherapy_measurements`: ROM 0–360°, MMT 0–5,
   NRS 0–10, versioned functional instruments; date, source/method,
   anatomical side, author and append-only correction rationale.
2. `physiotherapy_care_plans`: immutable, versioned objectives (SMART),
   measurable goals, frequency/duration, interventions/precautions and phase.
   Phase is checked against the current orthopedic plan.
3. `physiotherapy_exercises`: instructions, contraindications, anatomical
   region, optional HTTPS instructional video with rights metadata.
4. `physiotherapy_hep`: approved exercise sets/repetitions/frequency, author,
   current care-plan ID, and snapshot of orthopedic ROM/load restrictions.
5. `physiotherapy_hep_logs`: authored and append-only adherence observations.
   Patient self-report is documented as reported, never auto-interpreted as
   clinical recovery.
6. `physiotherapy_reports`: draft/final/amendment versions with preserved
   full snapshots of the linked therapy data. Amendments require a reason.

## Clinician safety
- Tauri commands take clinician identity from the authenticated Doctor
  session, never an actor ID provided by the React client.
- SQLite guards prevent silent UPDATE/DELETE of measurements, care plan
  versions, exercise catalog records, HEP orders/logs and clinical reports.
- A new HEP authorization requires the latest approved care plan. Changing
  orthopedic ROM/load restrictions or the care-plan version marks older HEP
  records for professional review and refuses further observations.
- The physiotherapy care-plan phase must match the approved source phase;
  phase advancement never occurs solely by elapsed calendar time.
- A closed orthopedic episode cannot receive new therapy sessions, plans,
  assessments or routine reports. Signed amendments may be recorded as
  distinct versions.
- Functional tests require an instrument name, version and unit; unlike
  standard ROM/NRS/MMT instruments, distinct units are not merged into
  a single chart.

## UI
`src/pages/PhysiotherapyClinicalWorkspace.tsx` adds:
- Anatomical selection, laterality and formal ROM/MMT/NRS/functional records.
- Versioned treatment plans, goals, frequency and phase.
- Exercise catalog, instructional video metadata, HEP prescription,
  exercises performed/not performed and status warnings.
- Three separate trend charts for ROM°, MMT 0–5 and NRS 0–10, with dated
  source points (not automated diagnoses).
- Signed versioned physiotherapy reports and native PDF export.
`src/pages/PhysiotherapyPage.tsx` retains the patient episode/session flow,
adds the clinical workspace, and provides native A4 referral export.
`src/pages/OrthopedicsPage.tsx` can export a rehab referral directly,
display progress reports and attach and open PDF reports in the clinical record.

## Native documents and transfer workflow
- `physiotherapy_pdf.rs` generates A4 referral and versioned report
  documents from real SQLite patient, protocol and session data. No
  `window.print()` or WebView screenshot.
- `physiotherapy_v3.sql` stores an immutable attachment registry with
  patient/episode FK checks, clinical author, PDF filename and SHA-256.
- Tauri's native picker validates PDF magic/EOF, maximum size 20 MB,
  copies the file into app data and verifies the checksum when reopening.
- Clinical data remain private unless a clinician explicitly saves and
  shares a document; the app does not transmit it automatically.
- Saved PDFs require appropriate institutional handling/consent. A PDF is
  not proof of qualified digital signature.

## Implementation files
Backend:
- `src-tauri/migrations/physiotherapy_v1.sql`
- `src-tauri/migrations/physiotherapy_v2.sql`
- `src-tauri/migrations/physiotherapy_v3.sql`
- `src-tauri/migrations/physiotherapy_v4.sql`
- `src-tauri/src/db.rs`, `lib.rs`, `physiotherapy_pdf.rs`

Frontend:
- `src/pages/PhysiotherapyPage.tsx`
- `src/pages/PhysiotherapyClinicalWorkspace.tsx`
- `src/pages/OrthopedicsPage.tsx`
- `src/api/client.ts`
- `src/App.tsx`, `src/departments.ts`, `src/components/Sidebar.tsx`

## Verification
New Rust regression cases cover: cross-patient referral refusal,
idempotent linking, closed-case write denial, ROM units/corrections,
versioned plans, media rights check, HEP current restriction checks,
compliance log validation, report immutability, patient-linked document
registration and A4 PDF bytes.

**The commits are real, but none of the new v2/v3/v4 changes has been
compiled or tested on Windows in this turn.** The earlier passing
50/50 Rust tests predate this implementation. Run:

```powershell
cd "C:\Users\Admin\repos\Mjeku\mjeku-ui"
git pull --ff-only
npm run typecheck
npm run build
cd "C:\Users\Admin\repos\Mjeku\mjeku-desktop"
git pull --ff-only
npm run cargo:test
```

Manual acceptance: 2 patients, 2 referrals, identical patient IDs
across linked views, phase and restrictions preserved, NRS/ROM/MMT
round-trip, approved care plan required for HEP, older exercises
flagged when source restrictions change, no patient crossover,
clinical report export, reference PDF import, SHA-256 verify,
closed orthopedic episode write refusal, and restart persistence.

## Still not production-complete
- Independently created physiotherapy care episodes without an orthopedic
  case link require a carefully migrated optional-source data model. This
  release does not silently fabricate orthopedic episodes for that purpose.
- A dedicated physiotherapist role and patient-specific consent/ACL policy
  are not implemented; current authenticated Doctor sessions are used for
  clinical actions. This is not equivalent to verifying professional
  licensing.
- No institutionally signed, interoperable digital clinical exchange
  or shared real-time status across installations has been validated.
  A saved PDF is a document handoff, not automatic data synchronization.
- Clinician-approved HEP content and range/phase safety still require
  review; software must not claim to prescribe exercises autonomously.
- Full PACS, implant inventory, cloud/offline clinical synchronization,
  independent Windows installer/macOS builds, backup/restore drills and
  healthcare regulatory acceptance remain separate release gates.

Do not use as a fully validated production medical device or clinical
record system until relevant security, audit, testing and sign-off steps
are completed.

## New addition: physiotherapy_v5.sql and reviewed progress charts

- `physiotherapy_body_markers` stores patient's anatomical findings,
  normalized (x,y), view, region, laterality, classification, author and
  time. Markers are append-only, and inserts are refused for closed cases.
- A real Body Map form in Physiotherapy uses the same anatomical SVG
  infrastructure as Orthopedics, while preserving its own clinical evidence.
- Trend charts group measurements by region, movement, laterality, method and
  instrument version; unrelated joint measurements are no longer combined.
  Superseded measurements remain in the audit history but are excluded from
  the plotted current clinical series.
- The existing HEP observation timeline now displays both completed and
  uncompleted exercises, authored dates and provenance of self-report.
- Physiotherapy's phase selector displays only the orthopedic-approved phase,
  instead of allowing a silent parallel phase change.
- An additional Rust test checks persisted Body Map coordinates and
  author metadata; another verifies idempotent application of migrations
  `physiotherapy_v1.sql` through `v5.sql`.

**No new Windows compiler, test suite, installer or end-to-end run has been
performed in this session.** Code and static command-registration checks only.
