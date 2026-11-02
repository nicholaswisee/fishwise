#!/usr/bin/env fish
# Nested scopes and literal argument contents.
function demo
    if set -qx greeting
        builtin true
        set -fx greeting 'hello'
    else
        builtin true
        set -fu greeting 'hello'
    end
    if test -n "$greeting"
        for item in one two
            printf '%s\\n' "$greeting $item"
        end
    else if false
        false
    else
        true
    end
    while false
        echo 'never
  literal indentation'
    end
    builtin true
    set -f greeting ''
    echo "$greeting"
end
demo
