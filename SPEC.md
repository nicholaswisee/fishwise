# fishwise (`fws`) — Bash Runner and Bash-to-Fish Converter

## Project overview

**fishwise** (`fws`) is a small Rust CLI for people who use **fish** as their interactive shell but regularly copy commands written for **Bash**. It solves the problem in two complementary ways:

1. **Run mode (already implemented, the default):** execute the supplied snippet with Bash, preserving Bash semantics. This is the reliable way to immediately run copied commands, heredocs, `.env` setup, arrays, and other Bash-specific features.
2. **Translate mode (to implement, opt-in):** convert a supported Bash command, pasted snippet, or script into readable fish syntax that can be reused in fish without invoking Bash.

**Keep the existing runner working exactly as users expect.** Translation extends `fws`; it does not replace Bash execution or silently change what `fws 'script'` does.

The tool should be local, lightweight, predictable, and usable from Linux and macOS. It should not require an internet connection, an LLM, or a background service.

## Existing implementation: preserve this behavior

The current project is a working Rust, Unix-only Bash runner. It already:

- Accepts one **complete Bash script as a single argument**, including `fws -- 'script'`.
- Reads the entire script from stdin when no argument is given. For interactive input, it prompts the user to paste and finish with **Ctrl-D** before execution.
- Supports `fws < setup.sh` and piped script input.
- Uses `std::process::Command` and Unix `exec()` to replace the `fws` process with `bash -c -- SCRIPT fws`, inheriting the working directory, exported environment, and standard streams.
- Preserves Bash's exit status and normal signal behavior through `exec`, without inserting `set -e` or changing shell options.
- Requires **Bash on `PATH`**; reports `127` when Bash is missing and `126` for other failures to start Bash.
- Rejects multiple positional script arguments instead of joining them and potentially changing their meaning.
- Uses the Rust standard library only, and explicitly rejects non-Unix platforms.

The existing commands and their behavior must remain valid:

```fish
fws 'NODE_ENV=production npm start'
fws 'echo ${NAME:-guest}; echo $((5 + 3))'
fws -- 'echo hello'
fws                    # paste Bash, Ctrl-D, then execute
fws < setup.sh          # execute the file content as Bash
fws --help
```

**Important boundaries:** running a Bash child cannot modify the parent fish session's variables or directory. Bash runs noninteractively (not as a login shell), and its own startup/environment rules still apply. Never describe `fws` as a sandbox or safe for untrusted scripts.

## New capability: opt-in translation

Users should be able to **inspect or save fish code instead of executing Bash**. Translation must **never run the supplied Bash commands**, even while analyzing or validating them.

### Proposed CLI

```fish
# Existing: execute with Bash (unchanged)
fws 'export A=1; echo "$A"'
fws < setup.sh

# New: output fish code without executing it
fws --translate -- 'export EDITOR=nvim'
fws --translate                       # paste multiline Bash, finish with Ctrl-D
cat setup.sh | fws --translate
fws --translate < setup.sh

# New: explicitly read a script file for conversion
fws --translate --file setup.sh
fws --translate --file setup.sh --output setup.fish

# New: check and explain conversion
fws --translate --file setup.sh --check
fws --translate --file setup.sh --explain
fws --translate --file setup.sh --strict
fws --translate --file setup.sh --validate
```

**CLI rules:**

- The default mode stays **run**, with no flags required. `--translate` (optionally aliased `-t`) explicitly switches to translation.
- In translation mode, accept **one script argument**, `--file PATH`, or **stdin** when neither is supplied. Do not guess whether a positional argument is a filename; treat it as script text, consistent with the existing runner.
- Keep `--` as the option terminator, so scripts beginning with `-` can be supplied unambiguously. Preserve quoting and argument boundaries; never join multiple command-line arguments to reconstruct a shell command.
- `--output PATH` (or `-o`) saves generated fish code; otherwise print code to stdout. The output flag applies only to translation mode.
- `--check` analyzes without writing generated code. `--explain` shows the important transformations and assumptions; `--strict` rejects uncertain translations; `--validate` checks emitted fish syntax without running it.
- Invalid option combinations and ambiguous/multiple input sources should produce a helpful error. `--help` should document both modes and clearly distinguish **execute** from **translate**.
- Paste mode always consumes the complete snippet before processing. Preserve the current runner's behavior regarding stdin: with piped/redirected input, stdin will already have been consumed by the runner when Bash starts.
- Clipboard integration, in-place editing, JSON output, and shell completions are optional later enhancements, not required for the first translator milestone.

### Example translation

Input Bash:

```bash
export PROJECT_DIR="$HOME/projects"
NAME="demo"

if [[ -d "$PROJECT_DIR" ]]; then
    echo "Found $NAME"
fi
```

Expected fish output:

```fish
set -gx PROJECT_DIR "$HOME/projects"
set -g NAME "demo"

if test -d "$PROJECT_DIR"
    echo "Found $NAME"
end
```

For a translated file, replace a recognized Bash shebang with an appropriate fish shebang. Do not add a shebang to a pasted command or stdin snippet unless the input already has one. Keep comments and reasonable formatting wherever possible.

## Translation design and safety

This is a **real transpiler**, not a list of regex substitutions. Bash and fish differ in variable scope, splitting, expansion, globbing, arrays, arithmetic, subshells, and more. Syntactically valid fish code is not automatically behaviorally equivalent Bash code.

**Core policy: translate when reliable, warn when approximate, reject when unsupported.** Do not silently modify the meaning of a script to make it look convertible. In particular, never fall back to *executing* the original snippet if translation fails; run mode is an explicit separate user choice.

Classify each transformation or relevant construct as:

- **Equivalent:** supported with no known semantic change under the stated assumptions.
- **Approximate:** a useful translation exists but may differ; warn on stderr, and reject it with `--strict`.
- **Unsupported:** no reliable implementation; fail the conversion and identify the construct and its source location. Do not output an apparently complete translated script.

Ordinary successful stdout must contain **only generated fish code**. Warnings, errors, and explanations belong on stderr. Failed conversions must use a nonzero status and must not emit partial executable code by default.

### Initial translation coverage

Implement these common features first, with syntax-aware handling and tests:

| Bash | fish / approach |
| --- | --- |
| `export NAME=value` | `set -gx NAME value` |
| `NAME=value` | `set -g NAME value` at top level; track scope elsewhere |
| `local NAME=value` | `set -l NAME value` in functions |
| `unset NAME` | `set -e NAME` where the scopes match |
| `echo "$HOME"` | Preserve if already compatible |
| `echo $?` | Use `$status`, taking care not to change status first |
| `if [ -f file ]; then ... fi` | `if test -f file ... end` |
| `if [[ -d "$DIR" ]]; then ... fi` | Convert supported test operators safely |
| `for x in a b; do ... done` | `for x in a b ... end` |
| `while ...; do ... done` | `while ... end` for supported conditions |
| `name() { ...; }` | `function name ... end` |
| Simple external commands, pipelines, comments | Preserve unless shell semantics require a change |

Support nested blocks and multiline input; parsing each input line independently is not sufficient. Avoid rewriting fish-compatible syntax needlessly. Be especially careful with variable assignments inside functions, temporary `VAR=value command` assignments, and external command arguments.

**Do not assume every Bash feature has a fish equivalent.** In the initial version, detect and reject or explicitly warn about cases such as:

- Unquoted variable expansion and implicit Bash word splitting.
- Arrays, associative arrays, positional parameters, and advanced `${...}` expansions not yet supported.
- Arithmetic with side effects or different numeric semantics.
- Heredocs, process substitution, command substitution with incompatible splitting, subshell state, and complex redirections.
- `source` / `. script.sh`, `.env` sourcing, `set -a`, Bash-specific options and traps when semantics cannot be preserved.
- Globbing differences and unmatched patterns.
- Bash code embedded inside string arguments to tools like `bash -c`, `ssh`, `sed`, or `python`—these are data, not necessarily outer-shell syntax.

This matters for this project's current use cases: **snippets containing `set -a`, `. ./.env`, and heredocs should continue to work immediately in run mode even if translate mode cannot handle them yet.** Do not weaken the runner to make the translation engine appear more capable.

Add advanced conversions incrementally with an explicit test for each supported pattern. Target a documented modern fish version (initially fish 4.x) rather than pretending to support all historic versions.

## Implementation guidance

**Build on the current Rust project instead of replacing it.** Preserve the Unix `exec()` runner and its zero-dependency behavior. Refactor only as needed to introduce a CLI mode dispatcher and a separate translation pipeline.

A suggested structure (adapt it to the existing repository):

```text
src/
  main.rs                 # CLI dispatch and top-level errors
  cli.rs                  # parse modes and arguments
  runner.rs               # existing bash -c + exec behavior
  translate/
    mod.rs                # translation orchestration
    parser.rs             # Bash parser with source locations
    analysis.rs           # compatibility and scope checks
    transform.rs          # syntax-aware conversions
    emit.rs               # readable fish output
    diagnostics.rs        # warnings and errors
    validate.rs           # optional fish -n check
```

For translation, prefer an established Bash parser/AST (for example, `tree-sitter-bash`) rather than attempting to write a complete Bash parser with regexes. It is acceptable to add **justified parser dependencies** for this new capability; however, keep the runner simple and avoid adding a large runtime framework or requiring network services. Update the README's “no Rust dependencies” claim if that is no longer true.

A typical translation pipeline is:

1. Read input as UTF-8 text; report invalid encoding rather than silently modifying it. This restriction applies to translation, not the existing runner's raw `OsString`/stdin handling.
2. Parse the *whole Bash snippet*, detect parse errors, and retain line/column information.
3. Analyze quoting, scopes, expansions, nested blocks, and unsupported constructs.
4. Transform supported structures with explicit rules, preserving external command arguments.
5. Emit deterministic, readable fish code, including comments where possible.
6. Optionally validate with `fish -n` (syntax-only) and deliver stdout or output file.

Validation must never execute the generated script. A successful `fish -n` check proves only fish syntax validity, **not equivalence**.

If writing an output file, do not overwrite an existing file unless the user explicitly supplies `--force`. Generate and verify the full output before replacing the destination; failures must leave the existing destination intact.

## Test coverage and completion criteria

Protect the current implementation with regression tests before expanding it:

- `fws '...'`, `fws -- '...'`, interactive stdin, piped stdin, and `fws < file` still execute through Bash.
- Variables and exports persist within one Bash snippet, not in the parent fish session.
- Multiline Bash, `.env` sourcing, heredocs, arrays, Bash expansions, and exit statuses retain the current runner's behavior.
- Multiple script arguments are rejected; `--help` works; missing Bash returns the existing error status.
- Translation mode **never executes** the input, even when it contains `touch`, `rm`, `$(...)`, or other side effects.
- Translation fixtures cover assignments, exports, quoting, tests, nested control flow, functions, comments, and unchanged external commands.
- Malformed/unsupported Bash produces source-located diagnostics and no misleading generated output.
- `--strict`, `--check`, `--output`, and `--validate` behave as documented; failed conversions never corrupt an output file.
- Supported outputs pass `fish -n` when fish is installed in the test environment. Compare behavior for selected **trusted, sandboxed** Bash/fish fixtures where practical.

**First milestone:** keep the existing runner stable, introduce `--translate` for a single snippet and stdin, implement basic AST-backed transformations, and reliably reject unsupported cases. Then add `--file`, `--output`, diagnostics/strictness, optional validation, and more language features.

## README expectations

The README should lead with the existing value proposition: **run copied Bash commands from fish without rewriting them**. Then introduce translation as an optional second workflow: **convert compatible commands into native fish syntax**. Show both side by side, describe when to choose each, and clearly say which translation features are supported.

Do not imply that `fws` changes the current fish session, converts all Bash syntax, or safely executes unknown commands.

---

**Guiding principle:** Keep Bash execution reliable and backward-compatible; make fish translation explicit, conservative, and increasingly capable.
