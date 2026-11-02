#!/bin/bash
# Nested scopes and literal argument contents.
demo() {
    local greeting='hello'
    if [ -n "$greeting" ]; then
        for item in one two; do
            printf '%s\n' "$greeting $item"
        done
    elif false; then
        false
    else
        true
    fi
    while false; do
        echo 'never
  literal indentation'
    done
    unset greeting
    echo "$greeting"
}
demo
