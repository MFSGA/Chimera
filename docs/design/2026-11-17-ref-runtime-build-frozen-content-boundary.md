# D2 — Ref runtime content capture at the build boundary

## Baseline and source comparison

- Chimera baseline `master` `2c966c59`; fixed `ref` gitlink `59724e85c34abb6995c22d101c876cc06dc16576`.
- Upstream reference: `ref/backend/nyanpasu-core/src/client/application_workflow/inputs.rs`, `client/runtime_build.rs`, `client/application_workflow/ports.rs`.
- Previous Chimera owner: `backend/tauri/src/client/application_workflow/inputs.rs` and `backend/tauri/src/state/profiles/actor.rs`; the production builder was `backend/tauri/src/enhance/runtime_builder.rs`.
- Shared owner: `backend/chimera-core/src/client/runtime_build_port.rs` now holds the active Profile dependency closure and `FrozenProfileContent`, while `client/runtime_build.rs` performs capture and build together on a blocking executor thread.

## Production changes

- `ProfilesActor::current_closure` delegates to the shared `current_profile_closure`, ensuring Profile mutation impact and runtime content capture use **one** dependency-closure algorithm. It retains the existing order: global transforms, selected profile, composition base/proxy contributors, then scoped transforms for the selected/config contributors.
- `application_workflow::inputs::FrozenProfileContent` is now the same shared type, re-exported at the original Tauri path to preserve target-identity serialization and existing call sites. Missing read results remain explicit errors; staged bytes overwrite only paths included in the dependency closure. Extraneous staged paths are not admitted.
- `RuntimeBuildService::build_with_capture` freezes the closure on the same `spawn_blocking` task that creates the script runner and builds the candidate. Both `build_candidate` and `build_with_capture` use the same `execute_candidate` implementation; no second builder or policy owner is introduced.
- The production `enhance::build_from_profiles_with_inspection` now calls `build_with_capture` with the existing filesystem source and staged bytes; it no longer uses the Tauri-specific lazy `CandidateProfileContentSource`. The shell still provides builtin-script selection and the runner factory.
- The pre-existing `RuntimeInputs::into_parts` returns the already frozen data for mutation candidates; passing those frozen entries as staged content into the new path keeps the exact captured document, even if the source file changes before the build. Uncaptured candidate reads remain explicit errors.

## Compatibility and safety

- No change to configuration schema, UI/IPC DTOs, runtime product serialization, CoreSpec binary selection, TUN/transparent-proxy validation, script order, strict/lenient transform failure classification, or submodule pointers.
- The pinned ref's `RuntimeBuildPort::publish` writes a product directly. Chimera must **not** bypass its candidate staging, endpoint validation, confirmed receipt and atomic promotion transaction. CoreSpec resolution and confirmed publication remain under the existing `CoreLifecycle` authority until the workflow ownership migration. This is a content-capture/build Port boundary, **not a claim that the full upstream RuntimeBuildPort has been copied**.
- Any future complete Port facade must map publication to that verified transaction, not introduce another mutable Core facade or independent writer.

## Regression coverage

- Shared Core tests check global/current/scoped dependencies, composition base and proxy contributors, stable closure ordering, staged override of failed source reads and rejection of uncaptured paths.
- Tauri's staged-file test now also deletes the file from the script-runner factory **after** capture and asserts the built document still uses the frozen value. This checks the capture-before-build ordering rather than only comparing output values.
- Run full `chimera-core` and Tauri unit tests, including Specta TypeScript binding freshness and existing Runtime Builder, Profile mutation, TUN and MMDB tests, plus pre-commit Rustfmt/Clippy.
- Real desktop E2E, privileged Service/TUN and host handoff remain independent acceptance gates.
