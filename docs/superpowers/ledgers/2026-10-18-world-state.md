# World-state migration ledger — Class-2 tests and deferred follow-ups

Plan: `docs/superpowers/plans/2026-10-18-rill-lang-world-state.md`
Spec: `docs/superpowers/specs/2026-10-18-rill-lang-world-state-design.md`
Branch: `feature/rill-lang-arrays` (plan base `6594221`; close-out `Task 8`)
Ledger file created by: Task 8, commit `docs(rill-lang): close world-state migration ledger and sweep docs`.

**Class-2 definition (plan, line 30):** a test that cannot be re-expressed against the
new API without changing its subject. Class-1 tests were migrated mechanically in the
commit that broke them; Class-2 tests were rewritten to compile and then either
`#[ignore]`d or deleted with an entry here.

**Reproduce the censuses:**

```bash
rg -n '#\[ignore' rill-lang/tests rill-adrift/tests   # live ignores
git log --diff-filter=D --name-only 6594221..HEAD -- rill-lang/tests rill-adrift/tests
git diff 6594221..HEAD -U0 -- rill-lang/tests rill-adrift/tests | rg '^-fn '
```

---

## 1. Class-2 tests ignored or deleted

### 1.1 Ignored at close-out — census

`rg -n '#\[ignore' rill-lang/tests rill-adrift/tests` → **9 ignores, all in `rill-lang`;
0 in `rill-adrift`; 0 in `rill-lang/src` / `rill-adrift/src`.** None was added by this
migration (Task 4 ledger: `git diff -U0 rill-lang | grep -E '^\+.*#\[(allow|ignore)'` → 0).
All nine predate the plan and are unchanged in subject and reason.

| # | File | Test | `#[ignore]` reason | Cause | Owner |
|---|---|---|---|---|---|
| 1 | `rill-lang/tests/ffi.rs:617` | `tape_param_maps_to_resource` | `resource/tape deferred: architectural issues` | Name-addressed `resource` params do not resolve on the source path; tapes resolve by index only (`BuiltinInstance::tape_index`) | FU-5 |
| 2 | `rill-lang/tests/ffi.rs:736` | `resource_param_wires_tape_ref` | same | same | FU-5 |
| 3 | `rill-lang/tests/ffi.rs:754` | `resource_param_rejects_non_ref_arg` | same | same | FU-5 |
| 4 | `rill-lang/tests/ffi.rs:1314` | `tape_loop_read_head_compiles_and_runs` | same | tape resources are architecturally deferred; Task 7 removed the legacy `ResourceRegistry` plumbing from `build_builtin`, leaving only the index path | FU-5 |
| 5 | `rill-lang/tests/ffi.rs:1348` | `tape_loop_float_capacity_is_rejected` | same | same | FU-5 |
| 6 | `rill-lang/tests/ffi.rs:1369` | `tape_loop_zero_capacity_is_rejected` | same | same | FU-5 |
| 7 | `rill-lang/tests/ffi.rs:1385` | `undeclared_resource_ref_is_reported_on_source_path` | same | same | FU-5 |
| 8 | `rill-lang/tests/func_values.rs:29` | `function_reference_calls_signal_definition` | `signal-lambda main: no unary-model equivalent (removed with the signal surface)` | asserts the `Value::Callable`/value-channel output of a func value over a signal function; the unary value model collapses it to the block track | FU-10 — not planned as a fix: re-expressing it needs a callable-output channel (signal-surface era) |
| 9 | `rill-lang/tests/func_values.rs:73` | `func_value_can_be_chained` | same | same | FU-10 (same) |

World-state migration status: **no ignored test was un-ignored, re-scoped, or
newly ignored**; the world-owned surface did not change any of their subjects.

### 1.2 Deleted during this migration (`6594221..HEAD`)

The plan anticipated Class-2 deletions in Task 4/5 (`set_param`-between-runs,
`value_outputs`-era asserts, multi-channel legacy). **None were needed**: the Task-4
ledger records zero deleted tests and zero new ignores — every test was Class 1
(mechanical `prog` → `world`, `(ir, &mut world)` helper migration). The only deletions
in the plan range are the two launch-argument tests and the graph-partition file.

| File | Test | Deleted in | Cause | Resolving item |
|---|---|---|---|---|
| `rill-lang/tests/io_runner.rs` | `main_string_args_bind` | `b82f5fd` (Task 0, finalizing the in-flight `main : IO ()` commit) | `main` became a pure `IO ()` value; `ProgramRunner::set_args` and String launch arguments were deleted, so `main a = …` no longer exists | not re-expressible: launch arguments are gone. `RackDef::args` survives as document-format compatibility only (see §2, Minor) |
| `rill-adrift/tests/modular_launch.rs` | `set_args_binds_main_string_parameters` | `b82f5fd` (Task 0) | same: rack launch no longer binds String parameters into `main` | not re-expressible; `launch` reports a non-`IO ()` `main` as `ModularError::Program` (covered by `launch_reports_a_non_io_main_as_a_compile_error`) |
| `rill-lang/tests/graph_partition.rs` (whole file, 84 lines) | `sine_graph_is_one_subprogram_with_nullbackend_input` | `6409361` (Task 7) | `rill_lang::graph`, its `graph` Cargo feature and the legacy `Registry` were deleted; there is no graph partition surface to test | FU-6 (graph work is deferred to a separate follow-up, not this plan) |

Task 7 also deleted the `src/graph/` module (7 files, 1819 lines). Verified with
`git show 6409361^:<file> | grep -c '#\[test\]'` → **0** in every one of the seven files,
so no in-source unit tests were lost there.

Rewrites that are **not** deletions (same subject, earlier phase or narrower surface;
recorded here because the name or assertion moved):

| File | Test | Change |
|---|---|---|
| `rill-lang/tests/io_runner.rs` | `non_void_main_result_is_a_clean_error` → `non_void_main_result_is_a_compile_error` | the `IO ()` requirement moved from the run-time latch to the compile gate (Task 4); still pinned, now at `compile` |
| `rill-lang/src/lib.rs` | `public_compile_*` names → `closed_caf_compiles` / `complete_mode_compiles` (Task 4; renamed again for grep hygiene in Task 6) | drive the private `compile_to_ir` pipeline; the public gate is pinned in `tests/compile_gate.rs` |
| `rill-lang/src/serde_def.rs` | `compile_def_runs` | runs an `IO ()` program and asserts captured stdout (the old test ran a value entry through the deleted `run_entry`) |
| `rill-lang/tests/graph_compile.rs` | `engine_with_param_compiles_and_processes` | live-parameter re-run half dropped; default-parameter single run retained |
| `rill-lang/tests/caf_free_variables.rs` | `shared_oscillator_runs_one_instance_per_tick` | second tick and the old two-channel `ch0 == ch1` agreement dropped; one-shared-instance call counter retained |
| `rill-lang/tests/hof.rs` | `compose_three_times_does_not_exhaust_arena` | 10-tick re-assert loop collapsed to the single run |
| `rill-adrift/tests/engine_params.rs` | `actor_param_default_reaches_a_block_builtin` | reduced to the default-silence half; live-parameter half listed below |
| `rill-adrift/tests/modular_launch.rs` | `launch_reports_a_non_io_main_as_a_compile_error` | renamed/retargeted: `main = 5.0` is now a launch-time `ModularError::Program`, not a deferred run error |
| `rill-adrift/tests/modular_launch.rs` | `launch_surfaces_a_run_error_through_the_case` | kept; the run fault is now an out-of-range `arrayIndexFlat` (a program that compiles but faults at run) |

### 1.3 Class-2 deletions inherited from earlier branch work (before the plan base)

These are **not** world-state deletions. They were removed on the same branch by the
earlier signal-surface / single-run-value migration (and by `4c17de9`, which dropped the
graph-reconstruction tests). They are listed because the test files still carry
"NOTE — DELETED" comments for them and because the ledger is the single record of every
Class-2 removal on the branch. Cause is common unless noted: **the subject required a
multi-tick / re-run signal surface that the single-run value model removed.**

| File | Test(s) | Cause / note |
|---|---|---|
| `rill-lang/tests/smooth.rs` | `smooth_ramps_toward_target_across_ticks` | cross-tick convergence observable; algorithm-level unit tests cover the ramp |
| `rill-lang/tests/branching.rs` | `if_switches_on_setparameter_between_ticks`, `if_and_runtime_match_combined`, `if_nested_in_expression_at_runtime` | `SetParameter`-driven per-tick switching |
| `rill-lang/tests/match_patterns.rs` | `match_over_sum_param_switches_with_setparameter` | runtime match scrutinee switched between ticks |
| `rill-lang/tests/main_cells.rs` | `set_param_writes_into_the_cell`, `param_reads_as_buffer_in_signal_context`, `main_param_reads_as_value_in_value_context` | value-from-`set_param` and cross-tick cell persistence |
| `rill-lang/tests/currying_wires.rs` | `chained_currying_dispatches_without_leak`, `signal_wire_argument_scales_block`, `caf_applied_at_two_sites_with_different_wires` | 10-tick curry leak loop; signal-wire `_` arguments; 2-in/2-out program |
| `rill-lang/tests/closures.rs` | `closure_call_does_not_leak_across_ticks`, `closure_output_survives_ticks` | cross-tick closure/arena accounting |
| `rill-lang/tests/data_records.rs` | `compound_value_output_survives_ticks`, `nested_record_output_survives_ticks`, `newtype_value_output_survives_ticks`, `sum_value_output_survives_ticks`, `value_output_survives_across_ticks` | cross-tick arena-accounting observables |
| `rill-lang/tests/collections_list.rs` | `consed_list_shares_source_elements_across_ticks`, `repeated_growth_ticks_do_not_leak_arena` | cross-tick sharing / arena growth |
| `rill-lang/tests/collections_capacity.rs` | `container_outputs_pin_subtrees_across_ticks`, `map_build_and_pin_across_ticks` | cross-tick pinning |
| `rill-lang/tests/collections_map_set.rs` | `tail_shared_source_elements_survive_across_ticks` | cross-tick sharing |
| `rill-lang/tests/func_values.rs` | `func_value_as_output_survives_ticks` | cross-tick output survival (the two surviving tests are the ignores in §1.1) |
| `rill-lang/tests/ffi.rs` | `digital_effects_delay_live_param_reaches_algorithm` | `SetParameter` routed into the algorithm across 3+ ticks |
| `rill-lang/tests/ffi.rs` | `inline_tape_loop_calls_are_distinct_tapes` | two-output program read via `copy_output_samples(1, …)`; at-most-one-output model |
| `rill-lang/tests/ffi.rs` (Task 13b list, comment only) | `combinator_sugar_binds_delay_lhs`, `combinator_sugar_binds_loop_operand`, `combinator_sugar_binds_merge_operand`, `combinator_sugar_binds_split_operand`, `combinator_sugar_binds_par_operand_with_two_signals`, `combinator_sugar_binds_gain_scalar_only_call`, `combinator_sugar_does_not_double_bind_positional_call`, `foreign_fn_multi_signal_wires_distinct_inputs`, `foreign_variadic_signal_merge_accepts_channels` | the `@`/`~`/`:`/`<:` combinator-sugar surface and multi-channel signal routing |
| `rill-adrift/tests/engine_params.rs` | `actor_param_set_param_affects_output`, `actor_param_persists_across_ticks`, `actor_param_reaches_a_block_builtin` (live half) | "no host re-run surface" **at removal time**; `ProgramRunner::run()` is repeatable now, so these are re-expressible on the runner path (or on the value path via a re-seat — see FU-9) — recorded test debt (FU-9), deliberately not re-added in a migration task |
| `rill-lang/tests/graph_reconstruct.rs` (file deleted in `4c17de9`) | `stereo_mixer_to_mono_consumer_compiles`, `free_input_port_becomes_a_wire` | graph-reconstruction surface dropped; graph work deferred (FU-6) |
| `rill-lang/tests/io_runner.rs` | `main_string_args_bind` | also listed in §1.2 — the only deletion that falls inside the plan range, caused by `main : IO ()` |

Replacement tests added in the same files (not deletions, but the surviving coverage for
a reduced subject): `smooth.rs::smooth_compiles_with_square_input` replaces the deleted
cross-tick ramp; `graph_compile.rs::engine_with_param_compiles_and_processes` keeps the
default-parameter run; `caf_free_variables.rs::shared_oscillator_runs_one_instance_per_tick`,
`hof.rs::compose_three_times_does_not_exhaust_arena`, `data_records.rs::record_flow_through_process`,
`collections_capacity.rs` / `collections_list.rs` / `collections_map_set.rs` keep the
single-run structural asserts.

---

## 2. Deferred follow-ups

### 2.1 Named follow-ups (FU)

| ID | Item | Source | Why deferred / what "done" looks like |
|---|---|---|---|
| **FU-1** | ChunkedVec page alignment + growable migration + default flip | Task 2 review (Important #1, Important #3) | `ChunkedVec::with_capacity` makes a short last chunk (`chunked.rs:64-72`), `push_chunk` appends a full page at `base = len` (`:77-82`), `split` maps that page's first index back to chunk 0 (`:86-89`) → `at()` indexes past chunk 0; `Arena::alloc` grow path (`arena.rs:130-140`) panics for non-power-of-two capacities. Fix: allocate the last chunk full / map by full-page boundaries, migrate `io_runner::bounded_memory_overflow_is_a_clean_error` to explicit `growable: false`, then flip `WorldConfig::default().growable` to `true` per plan. **Currently `growable: false` in default builds (`cfg!(feature = "growable-arena")`), so the panic is not on the default path.** |
| **FU-2** | Growable-policy reachability | Task 2 review (Important #2), Task 4 review (Minor) | The policy is parsed and stored but no public constructor accepts a caller `WorldConfig` (`compile` builds its own), so `growable` is dead state for hosts. Add a config-taking door (e.g. `compile_with_config`) and decide the fate of the `growable-arena` feature then. Task 4 partially resolved this by routing `compile` through `WorldConfig`, but not the host-facing half. |
| **FU-3** | Runner sample-rate pre-seed test (rill-lang) | Task 4 review (Minor #4) | No rill-lang test pins `compile(...)` → `world_mut().params.sample_rate = X` → `run()`. Exercised indirectly by `rill-adrift`'s suites and `examples/program.rs`. Add one test in `rill-lang/tests/` (runner path, rate-sensitive builtin). |
| **FU-4** | Rack sample-rate pre-seed test (rill-adrift) | Task 6 review (Minor #8) | `modular` pre-seeds `runner.world_mut().params.sample_rate = def.sample_rate` before `run()`; no test pins that the rack rate (not `0.0` and not 44.1k) reaches a rate-sensitive rack program. |
| **FU-5** | `ResourceRegistry` orphan / resource–tape restoration | Task 7 review (Minor #4); the 7 `ffi.rs` ignores in §1.1 | After Task 7 the legacy resource path is gone and `rill_core::buffer::ResourceRegistry` has **zero consumers** (public orphan). Either restore a resource surface deliberately (which would un-ignore the seven tape/resource tests) or deprecate/remove the orphan. Name-addressed resources are rejected on the source path; tapes resolve by index only. |
| **FU-6** | `rill-analyzer` graph port | Task 7 review (note), root `Cargo.toml` exclusion comment | `rill-analyzer` still imports the deleted `rill_lang::graph`; it is excluded from the workspace. It needs a port to `(Ir, WorldState)` inspection or a rewrite. Graph compilation/partitioning is likewise deferred here (the deleted `graph_partition` / `graph_reconstruct` subjects). |
| **FU-7** | `rill-lang/benches/lang_bench.rs` | Task 4 review (Concern #1), Task 6 review (Concern #2) | Still names `RillProgram` and `prog.process(..)`; it was **already broken at HEAD** (benches are outside the `--lib --tests` gate and not built by CI). Repair as an IO-program benchmark or delete. Not touched by this commit (outside the gate, pre-existing breakage). |
| **FU-8** | `drift` publish-pinned usage | Task 7 review (Concern #1) | `drift` is a separate repository pinned to published `rill-adrift`; it must move from `full_registry*` / `compile_runner` / `RillProgram` to `full_ffi*` / `compile` on the next release. Out of this workspace's reach. |
| **FU-9** | Runner-path parameter test debt (rill-adrift) | Task 6 review (Minor #3); §1.3 | The three `engine_params` `set_param`-between-runs tests are re-expressible on the now-repeatable `ProgramRunner::run()`. Restore them (or an equivalent) rather than leaving the module doc as the only record. **Cheaper route than an `IO ()` rewrite:** `WorldState::seat` is re-seatable (`rill-lang/src/program.rs:391-406`) and preserves a pre-supplied parameter bag (declared defaults seed only absent names, `:490-506`), while `ValueReadParam` reads the parameter store at run time (`rill-lang/src/backend/interp.rs:1630`) — so two `test_eval` calls with `set_param` between them are a valid value-path re-run that observes the new value. |
| **FU-10** | `func_values` callable-output ignores | Task 8 review (Minor #6); §1.1 rows 8-9 | `function_reference_calls_signal_definition` and `func_value_can_be_chained` (`rill-lang/tests/func_values.rs:29,73`) assert a `Value::Callable` output over a signal function; the migration's unary value model collapses that to the block track, so they cannot be re-expressed against the new API without changing their subject. Resolving item: either add a callable-output channel or delete the two tests deliberately. Recorded here so "not planned" cannot silently become permanent. |

### 2.2 Smaller deferred items (docs, coverage, polish)

- **`docs/src/reference/crates.md`** (mdBook site) still names `compile_runner` /
  `full_registry` / `compile_with_ffi` / `run_value`. Out of this commit's scope
  (Task 8 adds only the ledger under `docs/superpowers/`); fix in the docs-site pass.
- **`serde_def` value-result coverage** (Task 4 Minor #3): `compile_def` is gated to
  `IO ()`, so the old value-result path has no serialization test.
- **`main_cells.rs:30` NOTE-DELETED block** (Task 4 Minor #5) is stale after the
  name-addressed `set_param` migration; audit and reword with §1.3's record.
- **Modular launch: unregistered FFI factory** (Task 6 Minor #2) is now a run error, not
  a build-time error; unpinned.
- **`RackDef::args`** (Task 6 Minor) is a serialized, launch-ignored field kept for
  document-format compatibility; removing it is a format change.
- **`params_dirty` naming** (Task 3 Concern #2): one flag now carries two causes
  (`set_param` and declared-default pre-seed). Candidate rename:
  `params_pending_push`.
- **`set_param` pre-seat clamping** (Task 3 Concern #1): a value set before `seat` is
  stored unclamped (the declarations are not known yet); the clamp guarantee is
  documented as post-seat.
- **Missing-`main` error kind** (Task 0 Minor #1): lowering reports
  `CompileError::Unsupported("no inferred scheme for `main`")` rather than the old
  inference-time `Type` error.
- **`WorldState::config()`** (Task 2 Minor) has zero consumers; `fill_layout` is named
  for filling but also resets. Cosmetic.
- **Push-into-builtin parameter leg** (Task 5 Minor #1): the dirty-param push to a bound
  built-in is not observable through `ProgramRunner`'s public surface; covered by the
  value-track pin only.
- **`compile_gate.rs:57-60` comment** (Task 5 Minor #2) overclaims observability: the
  gate fires before `WorldState::new`, so registration is never observable. Assertion is
  valid; the comment should say so.

### 2.3 Items closed during Task 8 (and why the chosen fix)

- **Task 4 Minor #1 — `WorldState::reset` visibility.** *Not* demoted to
  `pub(crate)`: `reset` is the sole user of `RegionDelay::clear` (and of the
  `value_regs` watermark), so a crate-private `reset` makes both dead in a
  non-test lib build and the clippy gate reports `method 'clear' is never used` /
  `method 'reset' is never used`. Instead the raw argument is now documented as
  [`Ir::num_value_regs`] and the `pub` choice is justified in the doc. Zero
  warnings, no `#[allow]`, no API break.
- **Task 4 Minor #2 — stale `compile_with_*` test names.** Already renamed
  (`closed_caf_compiles`, `complete_mode_compiles`); `rg 'compile_with'` over
  `rill-lang/src` is clean.
- **Task 5 Minor #4 — loose seat-error assertion.** `world_errors.rs` now asserts
  the full discriminator `foreign built-in 'mystery' is not registered`
  (`program.rs:779`), which the resource branch (`foreign resource built-in '…'`)
  cannot match.
- **`sample_rate: f32` on `WorldState::init`.** The method took a raw rate, which
  let a host initialise built-ins at a rate that disagrees with
  `Params::sample_rate`. It now reads the field (`pub fn init(&mut self)`); there
  were no in-workspace callers. Remaining `sample_rate: f32` occurrences are the
  FFI factory-closure contract (`ffi.rs`), DSP algorithm `init`/constructors, and
  test fixtures — none is a lowering parameter.

[`Ir::num_value_regs`]: ../../../rill-lang/src/ir.rs
