# ORTHOPEDICS_AUDIT — repository evidence (2026-10-25)

Source: user-supplied 55-page **Ortopedi.docx**, v1.0 / P0.

Repositories audited:
- `kasystem/mjeku-ui`: active `feature/mjeku-pos-sef-integration-20261024`, WIP `wip/orthopedics-20261024`.
- `kasystem/mjeku-desktop`: `feature/sef-motor-fiskal`.

Before these commits: the desktop already contained orthopedic SQL v1/v2 and SQLite CRUD/tauri commands in `src-tauri/src/db.rs` and `src-tauri/src/lib.rs`. The prototype React page was isolated on the WIP branch and absent from the active SEF UI. The prototype cached episodes in app_settings and did not reload surgery/implant/rehab objects.

| Spec capability | Before | Now | Missing for clinical release |
|---|---|---|---|
| Multiple patient episodes | PARTIAL | SQLite/UI connected | episode discharge/closure state machine |
| Skeletal diagram | PARTIAL WIP | interactive SVG, 30 zones, normalized markers, anatomical sides | anatomist-validated bone segmentation/lateral detail |
| ROM/test/deformity | PARTIAL | stored assessment & validations | shared Physiotherapy engine, units, instrument/catalog version |
| Fracture follow-up | PARTIAL | longitudinal append-only follow-ups | classifications licensing, diagnosis evidence and multi-study comparisons |
| Radiology/PACS | PARTIAL | study reference links | configured PACS server, DICOM display, identity validation |
| Surgery/operations | PARTIAL | separate planned vs finalized performed operation with surgeon identity | full institutional checklist, consent document signing, OR scheduling |
| Implants | PARTIAL | status validation, sterile/expiration, serial uniqueness, event journal | shared inventory debit, UDI decoding, recall and revision workflow |
| Rehab | PARTIAL | protocol + clinician phase-review events | shared Physiotherapy sessions/HEP, versioned approved protocols |
| Reports | MISSING | versioned JSON clinical records, draft/final/amend | clinical PDF renderer, institution-approved templates |
| Security | PARTIAL | clinician/surgeon commands, local immutable tables | patient-level ACL, audit of every read, remote access policies |
| Synchronization | BUGGY for orphan orthopedic queue | prevent unsupported sync attempts | versioned clinic server schema, conflict UI, offline reconciliation |
| Platform | PARTIAL | Tauri/Rust and Vite paths | Windows/macOS compile and E2E checks needed |

Files touched: UI `src/pages/OrthopedicsPage.tsx`, `src/orthopedics/SkeletalInjuryMap.tsx`,
`src/App.tsx`, `src/components/Sidebar.tsx`, `src/departments.ts`, `src/api/client.ts`.
Desktop `src-tauri/src/db.rs`, `src-tauri/src/lib.rs`,
`src-tauri/migrations/orthopedics_v3.sql`.

**No claims of completed medical certification, E2E PACS, inventory transaction or passed Windows build.**
Clinical specialist validation is required before live patient use.
