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

## Test execution status
**Not run in this connected GitHub environment.**
No claim of passing Cargo/TypeScript tests, Windows build or medical
end-to-end validation. No workflow/CI was activated.

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
- Cargo test and TypeScript build in real local checkout.
- Upgrade an existing SQLite clinic database containing legacy episodes.
- Windows/macOS packaging and platform E2E.
- Clinical sign-off, PACS, implant inventory and physiotherapy integration.
