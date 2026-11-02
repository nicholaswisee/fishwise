builtin true
set -gx PROJECT_DIR "$HOME/projects"
builtin true
set -g NAME "demo"
if test -d "$PROJECT_DIR"
    echo "Found $NAME"
else if test -z "$NAME"
    false
else
    true
end
