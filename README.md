# fishwise (`fws`)

Run copied Bash commands from fish without rewriting them. By default, `fws` executes the complete script with Bash; this remains the right choice for arrays, heredocs, `.env` sourcing, and other Bash features.

```fish
fws 'echo ${NAME:-guest}; echo $((5 + 3))'
fws --translate -- 'export EDITOR=nvim'
```

Run mode requires Bash on `PATH`. `fws` is written in Rust and uses tree-sitter-bash for translation; building from source needs a C compiler and parser build dependencies. The project currently supports Unix (Linux and macOS).

## Translate supported snippets

Translation is opt-in and never executes the input. It accepts one script argument, a file, or stdin through EOF. File paths remain native OS paths while script text must be UTF-8:

```fish
fws --translate 'export EDITOR=nvim; echo "$EDITOR"'
fws --translate                         # paste Bash, then Ctrl-D on an empty line
cat setup.sh | fws --translate
fws --translate --file setup.sh --output setup.fish
fws --translate --file setup.sh --check --explain --validate
```

Options:

- `--file PATH`: read a script file explicitly. A positional argument is always script text, never a guessed filename.
- `--output PATH` / `-o PATH`: write converted code to a file. Existing destinations are refused unless `--force` is supplied.
- `--force`: atomically replace an existing regular destination. A destination symlink is refused even with force.
- `--check`: analyze without writing or printing generated code; cannot be combined with output.
- `--explain`: print conversion details to stderr. Warnings also go to stderr.
- `--strict`: reject translations that rely on approximate Bash/fish behavior.
- `--validate`: ask fish 4.x to check generated syntax with `fish --no-config -n`. This checks syntax only; it does not execute code or prove Bash/fish equivalence.
- `--`: end option parsing; the next argument is the script.

Supported constructs include scalar assignments/exports, simple `unset`, comments and recognized Bash shebangs, supported `if`/`elif` tests, literal `for` loops, supported `while` conditions, brace functions with `local`, simple foreground commands, and stateless command pipelines. Only `echo`, `printf`, `true`, and `false` are translated as shell builtins. Assignments are preceded by `builtin true` to preserve their successful Bash status; status-dependent assignment values are rejected.

Some Bash/fish differences (including list-vs-scalar, function lookup/scope, unset visibility, loop binding, pipeline status, and dynamic echo options) are warned as approximate and rejected by `--strict`. Output is published only after conversion and optional validation succeed. Input/output aliases are rejected, including symlink and hard-link aliases. Use a stable, user-controlled destination directory; the destination's parent directory is the filesystem trust boundary.

Arrays, positional/advanced expansions, arithmetic, substitutions, heredocs, complex redirections, source/dot, shell-option/builtin management, and unsupported shell operators or syntax are rejected without partial code. Dynamic echo arguments may be fish options even when the Bash scalar is data, so they are flagged approximate (and strict rejects them). Variable assignments/reads are limited to ordinary identifiers: shell-reserved names and `BASH*`, `COMP_*`, and `fish_*` names are rejected. Translation input must be UTF-8. For unsupported Bash, use the default runner, not translation.

Only ASCII spaces, tabs, and LF newlines may be ignored between syntax tokens. Unquoted escaped whitespace/continuations and control-character separators (including unquoted CRLF) are rejected rather than silently losing argument data. Quoted control characters, Unicode, multiline literals, and literal-only quoted continuations remain supported.

## Install

```fish
cargo install --path .
```

Make sure `~/.cargo/bin` is on your fish `PATH`:

```fish
fish_add_path ~/.cargo/bin
```

## Paste a multiline command

Run `fws` without arguments:

```fish
fws
```

Paste your Bash snippet, then press **Ctrl-D on an empty line** to run it. Nothing executes until input is finished. For example:

```bash
set -a
. ./.env
set +a
go tool templ generate
go run ./cmd/server
```

Or paste a heredoc exactly as written:

```bash
python - <<'PY'
print("Hello world")
PY
```

This mode avoids having to escape quotes or Bash syntax for fish. It also supports exports shared by subsequent commands in the snippet:

```bash
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk
export PATH="$JAVA_HOME/bin:$PATH"
java -version
```

## Run a quoted command

Pass the complete snippet as **one argument**, using single quotes to prevent fish from expanding Bash variables first:

```fish
fws 'NODE_ENV=production npm start'
fws 'PORT=3000; export PORT; printenv PORT'
fws 'echo ${NAME:-guest}; echo $((5 + 3))'
fws 'files=(a.txt b.txt); echo ${files[0]}'
fws 'export PATH="$HOME/bin:$PATH"; command -v go'
```

For snippets containing single quotes, use paste mode instead of juggling nested quoting. Use `fws -- 'script'` if the script would otherwise be interpreted as a CLI flag. `fws --help` prints usage.

**Why not `fws <unquoted Bash command>`?** Fish parses quotes, variables, redirections, and pipelines before `fws` can see them. A CLI cannot undo that parsing. Multiple command arguments are rejected rather than silently reconstructed into a different command.

You can also read a script from a file:

```fish
fws < setup.sh
```

## Behavior and boundaries

- Commands inherit your current directory, exported environment, and terminal streams. Fish-only functions and unexported variables are not available to Bash.
- Variables, exports, `cd`, and shell options persist **within that one snippet**, not in your parent fish session or future `fws` calls. A child process cannot change its parent shell's environment.
- Bash runs as a noninteractive shell, like `bash -c`, not a login shell. It does not normally read `.bashrc`; Bash's usual environment behavior, including `BASH_ENV`, still applies.
- Bash's exit status and signals are preserved. No `set -e` or other shell options are injected: a failed command does not automatically stop later commands. Add `set -e` or `&&` yourself if appropriate.
- In quoted-command mode, the program can read standard input normally. Paste mode reads the script through EOF before execution; terminal input can then be used by commands, but redirected or piped input is already consumed. Use quoted-command mode when supplying data on stdin.
- External tools such as Go, Python, Java, and npm must already be installed. This provides Bash semantics, not compatibility with every shell dialect or missing program.

**Only execute trusted snippets and `.env` files.** Sourcing `.env` executes it as shell code, not just a list of harmless settings. `fws` is not a sandbox and runs commands with your permissions.

## Develop

```sh
cargo test
FWS_REQUIRE_FISH=1 cargo test --test parity
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
python3 tests/interactive.py target/debug/fws
```

The required parity check needs Bash and fish 4.x on `PATH`; without `FWS_REQUIRE_FISH=1`, fish-dependent tests report a skip when fish is unavailable. The PTY check exercises interactive paste through EOF and needs Python 3 on Unix.
