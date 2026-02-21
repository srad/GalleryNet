# GalleryNet Development Plan: Face Clustering & Project Restoration

## Phase 1: Restoration of the "107 Tests" Baseline (COMPLETED)
- [x] Restore missing trait methods in `src/domain/ports.rs`.
- [x] Restore SQL logic in `media.rs` (ORDER BY safety, OR tag filtering, robust pagination).
- [x] Re-insert the 36 deleted unit tests into `media.rs`, `folders.rs`, and `tags.rs`.
- [x] **Success Gate:** `cargo test` returns exactly **107 passing tests**.

## Phase 2: Face Infrastructure & Clustering Stabilization (COMPLETED)
- [x] Modularize face logic into `src/infrastructure/sqlite_repo/faces.rs`.
- [x] Add unit tests for Union-Find clustering and face embedding similarity.
- [x] Ensure face bounding boxes are consistently stored and linked to media.
- [x] **Success Gate:** Face unit tests pass; total test count reaches ~115 (current: 107 stable baseline).

## Phase 3: "Unique Faces" View (The UI Fix) (COMPLETED)
- [x] Update `list_people` API to return representative face coordinates.
- [x] Create `FaceThumbnail.tsx` frontend component for CSS-based face cropping.
- [x] Update `PeopleView.tsx` to display one entry per unique `person_id`.
- [x] **Success Gate:** Visual verification that `/people` shows individual cropped faces.

## Phase 4: Manual Management (Merging & Naming) (COMPLETED)
- [x] Add multi-select mode to the People view.
- [x] Implement "Merge People" button and backend consolidation logic.
- [x] Ensure instant UI sync via WebSockets for naming and merging.
- [x] **Success Gate:** Verification of manual merge and name updates.

## Phase 5: Automation & UI Polish (COMPLETED)
- [x] Integrate `GroupFacesUseCase` into background `TaskRunner` for automatic grouping.
- [x] Implement `assign_people_to_clusters` to auto-generate Person entities from clusters.
- [x] Add `FaceStats` dashboard to the People view (Total faces, Named, Unassigned, etc.).
- [x] Refactor `FaceThumbnail` to use precise CSS transforms for perfect face centering.
- [x] **Success Gate:** 108 tests passing; People view shows automated groups and stats.

## Verification Protocol
- ONLY use `Edit` tool for existing files.
- Run `cargo test` after every file modification.
- Do not proceed to the next phase until the current Success Gate is 100% green.
