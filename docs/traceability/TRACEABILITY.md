# Traceability

| Requirement | Roadmap | Decision/interface | Implementation | Verification | PR/release | State |
|---|---|---|---|---|---|---|
| `FCLONE-DETECTION-001-FR-001` | `DETECTION-BASELINE` | ADR-0003 | `traversal::traverse` | `traversal::tests::finds_regular_files_only` | initial commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-002` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::run_scan` (`by_size`) | `pipeline::tests::unique_files_produce_no_groups` | initial commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-003` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::process_size_group` | `pipeline::tests::finds_duplicates_larger_than_sample_size`, `pipeline::tests::no_duplicates_when_only_prefix_matches` | initial commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-004` | `DETECTION-BASELINE` | ADR-0003 | `traversal::traverse` (`follow_links`) | `traversal::tests::skips_symlinks_by_default` | initial commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-005` | `DETECTION-BASELINE` | ADR-0003 | `traversal::is_excluded_by_filesystem_boundary`, `traversal::device_component` | `traversal::tests::filesystem_boundary_*` (4 cases), `traversal::tests::device_component_*` (2 cases) | gap-closure commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-006` | `DETECTION-BASELINE` | ADR-0001, ADR-0003 | `pipeline::run_scan` (`by_file_id`) | `pipeline::tests::hardlink_aliases_are_included_when_content_matches_another_file`, `pipeline::tests::standalone_hardlink_pair_is_not_reported_as_duplicate` | gap-closure commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-007` | `DETECTION-BASELINE` | ADR-0004 | `pipeline::process_size_group` (`filter(len > 1)`) | `pipeline::tests::unique_files_produce_no_groups`, `pipeline::tests::standalone_hardlink_pair_is_not_reported_as_duplicate` | initial commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-008` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::verify_representatives` | `pipeline::tests::verify_representatives_drops_entries_that_do_not_byte_match`, `pipeline::tests::verify_matches_true_still_reports_real_duplicates` | gap-closure commit | Implemented, Verified |
| `FCLONE-DETECTION-001-FR-009` | `DETECTION-BASELINE` | ADR-0004 | `traversal::traverse` (`on_error`), `pipeline::process_size_group`/`verify_representatives` (`report_error`) | `traversal::tests::traversal_errors_are_reported_and_do_not_abort_the_scan`, `pipeline::tests::read_failures_during_hashing_are_reported_and_do_not_abort_the_group` | gap-closure commit | Implemented, Verified — hashing-stage errors were silently dropped until the gap-closure commit; see ADR-0004 change note |
| `FCLONE-DETECTION-001-NFR-001` | `DETECTION-BASELINE` | ADR-0004 | `pipeline::ScanHandle` (`Iterator`) | `pipeline::tests::finished_event_is_always_last_and_reports_every_group` (contract-level: `Finished` is always terminal and every group precedes it), manual CLI smoke test | gap-closure commit | Implemented, Verified (contract only — no test measures wall-clock overlap between traversal and hashing; see roadmap's `DETECTION-STREAMING-OVERLAP`) |
| `FCLONE-DETECTION-001-NFR-002` | `DETECTION-BASELINE` | ADR-0001 | `pipeline::process_size_group` | `pipeline::tests::no_duplicates_when_only_prefix_matches` (indirect) | initial commit | Implemented |
| `FCLONE-DETECTION-001-NFR-003` | `DETECTION-BASELINE` | ADR-0002 | `io_pool::IoPool` + rayon `into_par_iter` | `io_pool::tests::*` (pool mechanics only, not a concurrency benchmark) | initial commit | Implemented, not benchmarked |

State legend: `Implemented, Verified` = a requirement with a dedicated test
exercising it directly. `Implemented` (no `Verified`) = code exists and
matches the requirement but only indirect/no test coverage exists yet.

All requirements previously flagged "needs dedicated unit test" are closed
as of the gap-closure commit. Remaining open items are non-functional and
tracked on the roadmap (`DETECTION-BENCHMARK`,
`DETECTION-STREAMING-OVERLAP`), not functional gaps.
