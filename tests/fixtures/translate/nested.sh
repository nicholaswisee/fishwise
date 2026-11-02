export PROJECT_DIR="$HOME/projects"
NAME="demo"
if [[ -d "$PROJECT_DIR" ]]; then
    echo "Found $NAME"
elif [ -z "$NAME" ]; then
    false
else
    true
fi
