# Traceability

| Requirement | Roadmap | Decision/interface | Implementation | Verification | PR/release | State |
|---|---|---|---|---|---|---|
| `FCLONE-DETECTION-001-FR-001` | `DETECTION-BASELINE` | ADR-0003 | `traversal::traverse` | `traversal::tests::finds_regular_files_only` | initial commit | Implemented |
| `FCLONE-DETECTION-001-FR-002` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::run_scan` (`by_size`) | `pipeline::tests::unique_files_produce_no_groups` | initial commit | Implemented |
| `FCLONE-DETECTION-001-FR-003` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::process_size_group` | `pipeline::tests::finds_duplicates_larger_than_sample_size`, `pipeline::tests::no_duplicates_when_only_prefix_matches` | initial commit | Implemented |
| `FCLONE-DETECTION-001-FR-004` | `DETECTION-BASELINE` | ADR-0003 | `traversal::traverse` (`follow_links`) | `traversal::tests::skips_symlinks_by_default` | initial commit | Implemented |
| `FCLONE-DETECTION-001-FR-005` | `DETECTION-BASELINE` | ADR-0003 | `traversal::traverse` (`device_component`) | none yet | initial commit | Implemented, untested |
| `FCLONE-DETECTION-001-FR-006` | `DETECTION-BASELINE` | ADR-0001, ADR-0003 | `pipeline::run_scan` (`by_file_id`) | `pipeline::tests::finds_duplicate_small_files` (exercises hardlink path via CLI smoke test, not a unit test) | initial commit | Implemented, needs dedicated unit test |
| `FCLONE-DETECTION-001-FR-007` | `DETECTION-BASELINE` | ADR-0004 | `pipeline::process_size_group` (`filter(len > 1)`) | `pipeline::tests::unique_files_produce_no_groups` | initial commit | Implemented |
| `FCLONE-DETECTION-001-FR-008` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::verify_representatives` | none yet | initial commit | Implemented, needs dedicated unit test |
| `FCLONE-DETECTION-001-FR-009` | `DETECTION-BASELINE` | ADR-0004 | `traversal::traverse` / `pipeline::run_scan` (`on_error`) | none yet | initial commit | Implemented, needs dedicated unit test |
| `FCLONE-DETECTION-001-NFR-001` | `DETECTION-BASELINE` | ADR-0004 | `pipeline::ScanHandle` (`Iterator`) | manual CLI smoke test | initial commit | Implemented, needs automated test |
| `FCLONE-DETECTION-001-NFR-002` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::process_size_group` | `pipeline::tests::no_duplicates_when_only_prefix_matches` (indirect) | initial commit | Implemented |
| `FCLONE-DETECTION-001-NFR-003` | `DETECTION-BASELINE` | ADR-0002 | `io_pool::IoPool` + rayon `into_par_iter` | `io_pool::tests::*` (pool mechanics only, not a concurrency benchmark) | initial commit | Implemented, not benchmarked |

State legend: `Implemented` = code exists and matches the requirement;
"needs dedicated unit test" flags requirements only exercised indirectly
(CLI smoke test, other tests' side effects) — closing those gaps is fair
game for the next delivery-loop unit.
