# Orthopedics — episode discharge and reopening (2026-10-25)

## Source of truth and security
This work implements the missing orthopedic episode lifecycle from **Ortopedi.docx**.
The native SQLite clinic database is the source of truth. The React page is a
presentation layer and must not be trusted for authorization. The transition
command derives the clinician ID from the authenticated Tauri **Doctor** session.
Owners cannot medically sign or reopen an episode through the lifecycle command.

### Close an episode
1. Select the correct patient and orthopedic episode.
2. Complete any findings, injury markers, fracture reviews, procedures,
   implant status and rehabilitation notes while the episode is **active**.
3. In **Raportet**, write a clinically justified **Përmbledhja finale**
   (`final_summary`), set the status to **Final**, and save the signed version.
   The report uses the existing versioned `orthopedic_report_versions` record.
4. In **Përmbledhje** or **Raportet**, provide a reason of 10–2000 characters,
   then click **Mbyll episodin**.
5. The backend verifies status=active, existence of a signed final summary,
   and doctor's authenticated identity. It changes status to **closed** and
   writes an immutable event in a single SQLite transaction.
6. All orthopedic clinical tables reject INSERT, UPDATE and DELETE when the
   related episode is closed. Existing records remain readable. Signed
   report **amendments** with a reason remain possible; draft/new final reports
   do not.

### Reopen an episode
1. Select a closed episode.
2. Provide the clinical justification (10–2000 characters).
3. The authenticated doctor clicks **Rihap episodin**.
4. The backend changes status to **active** and records an additional
   append-only event. Original discharge reports and transitions are not
   deleted, edited, or overwritten.

### Data model
`src-tauri/migrations/orthopedics_v4.sql` introduces:
- `orthopedic_episode_transitions`, including actor, rationale, prior/new
  status, optional signed final report ID and timestamp.
- Immutable transition triggers (no UPDATE or DELETE).
- Database-level closed-episode triggers for direct and indirectly linked
  orthopedic clinical tables, including implants and rehabilitation reviews.

Rust API in `src-tauri/src/db.rs`:
- `orthopedic_episode_transition(episode_id, action, reason, clinician)`
- `orthopedic_episode_transitions_list(episode_id)`
Tauri API in `src-tauri/src/lib.rs`: corresponding commands, with session checks.
UI: `src/pages/OrthopedicsPage.tsx` and `src/api/client.ts`.

### Boundaries and known outstanding work
- A signed report is not equivalent to an institution-approved discharge form.
  Clinicians remain responsible for verifying findings and follow-up plan.
- Orthopedic data still stay local: versioned server schema and conflict-aware
  synchronization have not been implemented.
- DICOM/PACS display, shared implant inventory debit/recall, physiotherapy
  session integration, and dedicated PDF report export are still incomplete.
- Platform and clinical end-to-end validation must be completed before any
  release for actual patient care.
