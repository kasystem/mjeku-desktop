# Ortopedi ↔ Fizioterapi: first working integration (2026-10-25)

## Source
The supplied `Ortopedi.docx` demands a shared Physiotherapy engine:
referral from an orthopedic episode, an approved rehabilitation protocol with
restrictions, longitudinal sessions and ROM/MMT tracking, physiotherapist
reports, and surgeon review. The same clinical patient must be reused. The
separate `Fizioterapi.docx` did not previously exist; the new specification is
`PROGRAMERI_MJEKU_Fizioterapi_Specifikim_Integrim_Ortopedi.docx`.

## What the committed first phase implements
- `physiotherapy_episodes` table references existing `clients`,
  `orthopedic_episodes`, and `orthopedic_rehab_plans`, with one
  idempotent referral per `rehab_plan_id`.
- SQLite rejects referrals whose plan does not match the same active
  orthopedic episode/patient; no separate patient database is introduced.
- Ortopedi → Rehabilitimi: **Dërgo në Fizioterapi** appears for each
  existing orthopedic rehab plan. A second click cannot create a duplicate
  linked clinical episode.
- Fizioterapi workspace: lists referrals and pulls protocol, phase, diagnosis,
  laterality and current load/ROM restrictions from the original orthopedic
  tables. Restrictions are read-only.
- Fizioterapi: stores dated, authored treatment sessions with intervention,
  response, before/after pain NRS 0–10, optional ROM 0–360°, MMT 0–5 and next
  plan in one append-only SQLite table.
- Ortopedi → Rehabilitimi: reads the exact same therapy sessions and gives
  the orthopedic clinician a view of linked rehabilitation progress.
- The backend refuses a new session when linked orthopedic episode is closed,
  including via a direct SQLite trigger. Sessions cannot be overwritten or
  deleted silently.

## Existing architecture, not rebuilt
- Frontend: `kasystem/mjeku-ui` branch
  `feature/mjeku-pos-sef-integration-20261024`.
- Backend: `kasystem/mjeku-desktop` branch
  `feature/sef-motor-fiskal`.
- Migration: `src-tauri/migrations/physiotherapy_v1.sql` is additive and
  `CREATE TABLE/TRIGGER/INDEX IF NOT EXISTS`.
- Database API: `physiotherapy_referral_create`,
  `physiotherapy_episodes_list`, `physiotherapy_sessions_list`, and
  `physiotherapy_session_add`.
- All four Tauri commands verify existing orthopedic-clinician auth;
  writes require a logged-in Doctor session, with clinician_id derived
  from session state, never supplied by the client.
- UI screens: `OrthopedicsPage.tsx` and `PhysiotherapyPage.tsx`;
  licensing and sidebar routes are connected.

## Important limitations
- **Not a completed standalone physiotherapy specialty.** Only patients
  referred by an existing orthopedic rehabilitation plan enter the v1
  Physiotherapy workspace. Standalone physiotherapy cases, exercise videos,
  HEP catalog/adherence, functional outcome questionnaires, versioned
  physiotherapy reports/PDF and full progress charting are not implemented.
- **Dedicated physiotherapist role is missing.** Existing Doctor sessions
  are used to protect clinical write actions, not to assert that they
  represent credentialed physiotherapists. Patient-level ACL also remains
  incomplete; before real clinical deployment, verify both roles and
  clinic/patient assignment server-side.
- ROM/MMT within the session are initial structured metrics, not a validated
  shared instrument catalog with measurement provenance and phase-specific
  clinical range rules. A therapist must interpret them.
- Surgeon-controlled phase approval remains in the existing
  `orthopedic_rehab_phase_reviews` append-only workflow; phase is not
  auto-advanced from therapy measurements.
- Offline sync to multi-device/cloud is not implemented for new tables.
  Do not claim multi-device availability until a versioned server schema
  and conflict-aware reconciliation are tested.
- Closed-case guards prevent new sessions, but explicit continuation
  under a new independent episode needs a future approved workflow.

## Manual acceptance checklist
1. Sign in as Doctor, create a test patient and an orthopedic episode.
2. In Ortopedi/Rehabilitimi save a plan with **load restriction** and
   **ROM restriction**.
3. Click **Dërgo në Fizioterapi**; verify one referral and no duplication.
4. Open Fizioterapi and select that same patient/diagnosis/protocol.
5. Confirm restrictions match exactly without retyping or modifying.
6. Register a session (pain 5 → 3, ROM 50° and MMT 3).
7. Return to Ortopedi/Rehabilitimi and verify the session appears.
8. Repeat with a second patient: no session or protocol crossover.
9. Try invalid NRS/MMT/ROM and mismatched plan: backend must refuse.
10. Close the orthopedic episode after a signed final report. Verify a
    subsequent physiotherapy session is refused.
11. Restart the Windows desktop and confirm the data persist.
12. Verify `npm run typecheck`, `npm run build`, `npm run cargo:test`,
    then manually inspect UI and Windows .exe packaging.

**Code is committed, but current new Physiotherapy changes are not yet
confirmed by a user-provided Windows build/test run. No CI and no merge.**
