# Bash-to-Fish Translation Implementation Plan

> **Completion:** All six implementation tasks are complete and verified on Linux; macOS was not tested. The implementation was initially left uncommitted; the user later authorized incremental commits and a local merge to `main`. No push was requested. Deliberate limits, status-reset behavior, and rejection of unquoted escaped gaps are documented in the README.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task after approval. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the Bash runner while adding explicit, conservative, AST-backed translation into fish 4.x.

**Architecture:** Isolate the existing std-only runner from CLI dispatch and the translation pipeline. Parse complete snippets with tree-sitter-bash, analyze and emit through an exhaustive AST walker, and buffer the result before validation or delivery. Separate analysis from emission only if their complexity warrants it; no speculative intermediate framework.

**Tech Stack:** Rust 2021, Unix standard library, compatible stable `tree-sitter` and `tree-sitter-bash` releases pinned through Cargo.lock, optional fish 4.x syntax validation.

**Spec:** `SPEC.md` (user-owned; do not modify or stage without permission).

## Global Constraints

- “Keep the existing runner working exactly as users expect.”
- “Translation must never run the supplied Bash commands, even while analyzing or validating them.”
- “Core policy: translate when reliable, warn when approximate, reject when unsupported.”
- “Ordinary successful stdout must contain only generated fish code.”
- “Failed conversions must use a nonzero status and must not emit partial executable code by default.”
- “Do not overwrite an existing file unless the user explicitly supplies `--force`.”
- Preserve Unix `exec()`, raw runner arguments/stdin, 127/126 startup errors, streams, signals, working directory, and environment.
- UTF-8 restrictions apply only to translation, not run mode or filesystem path names.
- Target Linux/macOS and fish 4.x. No internet/runtime service, LLM, CLI framework, or execution fallback.
- Parser dependencies change the binary's dependency claim; the runner module itself remains std-only.
- The original implementation request did not include commits or pushes; the user later authorized incremental commits and a local merge to `main`. Do not push without authorization.

## Review Focus

- Legacy flag-shaped scripts and `--` must not accidentally acquire translator option semantics (Task 1).
- Ambient fish lists, export attributes, dynamic caller scope, and special variables can invalidate scalar assumptions (Tasks 2 and 5).
- Helper commands, loop variables, and pipeline stages can change `$?` or shell state (Task 5).
- Existing destinations, symlinks, path aliases, and concurrent output creation must not cause data loss (Task 3).
- Literal embedded programs and substitutions must not be rewritten or executed during translation/validation (Tasks 2, 4, and 6).

## Repository Baseline

- `src/main.rs`: 55-line runner; `tests/cli.rs`: 11 existing passing integration tests.
- No current dependencies, translator, or library target. README documents runner-only usage.
- Work on an isolated feature workspace at execution time; preserve the original untracked `SPEC.md`.
- Each task has exclusive ownership of its listed files while active. Handoffs are sequential where files overlap.

## Shared Interfaces and Policy

- `cli::parse(Vec<OsString>) -> Result<Invocation, String>`.
- `Invocation::{Help, Run(Option<OsString>), Translate(TranslateArgs)}`.
- `Source::{Stdin, Script(OsString), File(PathBuf)}`.
- `TranslateArgs { source: Source, output: Option<PathBuf>, force: bool, check: bool, explain: bool, strict: bool, validate: bool }`.
- `runner::run(Option<OsString>) -> !`: existing raw input and Bash exec behavior.
- `translate::translate(&str) -> Result<Translation, Vec<Diagnostic>>`: pure transformation; no process/filesystem access.
- `Translation { code: String, diagnostics: Vec<Diagnostic> }`.
- `Diagnostic { classification: Classification, span: Span, message: String }`; classification is Equivalent, Approximate, or Unsupported.
- `Span { start: usize, end: usize, line: usize, column: usize }`: byte offsets and one-based line/byte-column locations.
- `parser::parse(&str) -> Result<tree_sitter::Tree, Vec<Diagnostic>>` and `emit::emit(&str, &tree_sitter::Tree) -> Result<Translation, Vec<Diagnostic>>`.
- `translate::run(TranslateArgs) -> i32`: processing failures return 1; CLI usage failures return 2.
- Only leading `--translate`/`-t` selects translation. Existing `-- 'script'` bypasses mode detection; other legacy single arguments remain scripts. Inside translation mode, reject unknown options and multiple sources.
- `--check` allows explain/strict/validate but rejects output/force; force requires output. Duplicate file/output options are errors.
- Classify assumptions explicitly: known ordinary scalar values may be equivalent; ambient list/scope assumptions require warnings and strict rejection. Special variables, unknown AST constructs, and unimplemented builtins fail closed.
- Treat embedded literal scripts as argument data. Re-encode parsed literal segments when Bash/fish quoting differs; never perform blanket replacements inside strings.

## Task 1: Protect and Extract the Runner

**Files:** `src/main.rs`, new `src/cli.rs`, new `src/runner.rs`, `tests/cli.rs`, new `tests/interactive.py`.
**Interfaces:** CLI Run/Help variants and `runner::run`; translation arrives in Task 2.

- [x] Add passing characterization tests before refactoring: legacy flag-shaped single scripts still reach Bash; escaped `--translate` is script text; multiple inputs/lone `--` fail with 2.
- [x] Add raw invalid-UTF8 argv/stdin, cwd/environment, file-redirection stdin, consumed-input EOF, non-executable Bash (126), and signal regressions. Keep all existing tests.
- [x] Add Python-stdlib PTY checks proving prompt, no execution before EOF, multiline execution, and usable terminal input after EOF.
- [x] Run `cargo test --test cli` as the pre-refactor baseline. New behavioral fixes, if needed, require an observed failing test first; characterization tests need not artificially fail.
- [x] Extract the runner with no change to `bash -c -- SCRIPT fws` or non-Unix rejection.
- [x] Verify `cargo test --test cli && cargo build && python3 tests/interactive.py target/debug/fws`.

## Task 2: First Translator Milestone

**Files:** CLI/main, Cargo files, new `src/translate/{mod,parser,emit,diagnostics}.rs`, new `tests/translate_cli.rs`, README.
**Interfaces:** Shared translation types; initially script/stdin input and stdout output.

- [x] Write failing tests for mode dispatch, escaped script arguments, complete stdin, empty input, and invalid/multiple arguments.
- [x] Assert `export EDITOR=nvim` emits `set -gx EDITOR nvim`; ordinary assignment emits `set -g`; quoted values retain spaces, empty arguments, escaped dollars, and literal embedded code.
- [x] Test comments, recognized Bash shebang replacement, no invented shebang, malformed multiline locations, invalid UTF-8/NUL rejection, and zero stdout after a later unsupported construct.
- [x] Test that `touch` is emitted without execution and `$(touch ...)` is rejected without side effects. Unknown syntax, temporary assignments, special variable semantics, and unsupported builtin forms must fail closed.
- [x] Run `cargo test --test translate_cli` and observe the missing translator failures.
- [x] Add compatible stable parser dependencies, inspect actual AST node kinds/fields, and parse the complete snippet. Reject ERROR/MISSING nodes before emission.
- [x] Implement syntax-aware literal words, supported scalar assignments/exports/quoted reads, simple external commands, and explicitly tested echo/printf forms. Emit approximate warnings immediately where assumptions are needed.
- [x] Verify `cargo test`; update README with the working first milestone and revised dependency claim.

## Task 3: File Input and Safe Output

**Files:** CLI, `src/translate/mod.rs`, new `src/translate/io.rs`, translation CLI tests.
**Interfaces:** `io::read_source(&Source) -> Result<String, String>`; `io::publish(&Path, &[u8], bool) -> std::io::Result<()>` (the bool is force).

- [x] Write failing tests for file/script/stdin exclusivity, non-UTF8 filenames, missing input, duplicate options, code-only output, and untouched input.
- [x] Test existing files/dangling symlinks without force, observed symlinks with force, and identical/aliased/hardlinked input-output paths. Reject in-place conversion even with force.
- [x] Test competing no-clobber publishers: exactly one succeeds and its complete bytes survive. Test staged-write/publication failures leave any prior destination unchanged.
- [x] Run the new focused tests and confirm failures before implementing file delivery.
- [x] Complete decoding/conversion/validation before publication. Stage via an exclusive mode-0600 file in the destination directory; finish writes and sync before publication.
- [x] Publish without force using atomic hard-link creation, or with force using atomic rename; never open/truncate the destination or follow its symlink target. Clean staging on errors; successful publication is the commit point.
- [x] Verify the full suite. Document stable, user-controlled parent directories as the filesystem trust boundary; cleanup failure after successful publication is a warning.

## Task 4: Check, Explain, Strictness, and Validation

**Files:** CLI, translation orchestration/diagnostics, new `src/translate/validate.rs`, translation CLI tests, README.
**Interfaces:** `validate::syntax_check(&str) -> Result<(), String>`.

- [x] Write failing tests: check emits no code/file; explain uses stderr; approximate succeeds with warning normally and fails without output under strict; unsupported always fails.
- [x] Test ambient HOME/scalar assumptions and special/list variables; diagnostics must identify location and the actual assumption.
- [x] Test missing fish, rejected fish syntax, destination preservation after validation failure, and large input. A controlled validator executable records argv/stdin to verify syntax-only invocation and isolated input.
- [x] Run focused tests and observe failures before implementing options.
- [x] Apply strictness before delivery. Invoke `fish --no-config -n`, piping only generated code, concurrently feeding/draining to prevent pipe deadlocks; close stdin and reap the process.
- [x] Support check+validate; missing fish fails clearly. Validation must never source or execute code and must not claim semantic equivalence.
- [x] Verify `cargo test` and document flag combinations and validation boundaries.

## Task 5: Nested Language Constructs and Scope

**Files:** parser/emitter/diagnostics, translation tests, new `tests/fixtures/translate/` golden pairs for nested blocks and functions.
**Interfaces:** Extend the existing emitter and internal binding/scope analysis without changing the public transformation API.

- [x] Write failing goldens for the SPEC example, nested if/elif/else, supported `[ ... ]`/`[[ ... ]]` tests, literal-list for loops, while, functions, locals, assignments, unset, and comments.
- [x] Add negative tests for regex/pattern comparisons, arithmetic loops, globs, state-mutating pipeline stages, and ambiguous scope; each must reject or carry a precise approximation warning.
- [x] Test `$?` after false and both pipeline orderings; no emitted helper may change status before its corresponding `$status` expansion.
- [x] Test function locals inside nested blocks, updates to known locals, inherited export attributes, missing-variable unset, and caller-local shadowing. Warn/reject where binding cannot be established.
- [x] Run focused tests before implementation. Recursively emit structured fish blocks; handle test operands according to their AST/quoting context, not as generic Bash words.
- [x] Support only ordinary/stateless pipelines with last-stage status. Use function scope for nested local declarations where fish block-local scope would differ; preserve or explicitly classify loop-variable/scope differences.
- [x] Implement scope-aware unset only where status and binding behavior can be preserved; reject unresolved/special bindings rather than hiding erase failures.
- [x] Verify `cargo test`; unsupported Bash must still work through the unchanged run mode.

## Task 6: Adversarial Coverage, Parity, and Documentation

**Files:** translation tests, new `tests/parity.rs`, README; focused production fixes returned to the responsible implementation task.
**Interfaces:** `FWS_REQUIRE_FISH=1` makes absent/non-4.x fish a parity-test failure; otherwise report explicit skips.

- [x] Add negative tables for arrays, positional/advanced expansions, arithmetic, substitutions, heredocs, complex redirections, source/dot, set/shopt/trap/eval/exec, and unknown syntax.
- [x] Test side-effect sentinels across normal/check/explain/strict/validate modes, with failures after convertible prefixes; assert zero generated stdout and unchanged destinations.
- [x] Add trusted fixed parity fixtures comparing stdout/status and argument boundaries under Bash and fish in private temporary directories with controlled environments. Never execute rejected/untrusted input for parity.
- [x] Add fish syntax checks for every supported golden, including comments/shebangs, nested scopes, empty values, Unicode, and literal embedded programs.
- [x] Lead README with Bash execution, then show translation beside it; document supported constructs, strictness, assumptions, parser/build dependencies, fish 4.x, output safety, and non-goals.
- [x] Verify `cargo test`, `FWS_REQUIRE_FISH=1 cargo test --test parity`, `cargo build`, `python3 tests/interactive.py target/debug/fws`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`.
- [x] Obtain independent correctness/spec review, resolve actionable findings, rerun integrated checks, and report remaining platform/semantic limitations.

## Scope and Approval

Tasks 1–2 deliver the first milestone; Tasks 3–6 complete the remaining specified initial workflow. Clipboard integration, JSON, completions, in-place editing, and advanced Bash language conversions are deferred.

Conversion/validation failures emit no code; a later OS stdout write failure cannot retract already-written bytes. No separate dependency-free binary build is proposed. Approval originally authorized implementation, not commits or publication. The user subsequently authorized incremental commits and a local merge to `main`; pushing remains unauthorized.
