#!/bin/sh
# Read-only selection: never create a missing mount or move an existing cache.
set -eu
if [ "$#" -ne 3 ]; then
    printf '%s\n' 'usage: cargo-target-dir.sh PROJECT CHECKOUT BUILD_VOLUME' >&2
    exit 2
fi
project=$1
checkout=$2
volume=$3
case "$project" in magicvault|magicrun) ;; *) exit 2 ;; esac
case "$checkout" in /*) ;; *) exit 2 ;; esac
case "$volume" in /*) ;; *) printf '%s\n' "$checkout/target"; exit 0 ;; esac
# Trailing slashes must not turn a missing volume into its writable parent.
while [ "$volume" != / ] && [ "${volume%/}" != "$volume" ]; do volume=${volume%/}; done
target="$volume/$project/builds"
if [ "$volume" != / ] && [ -d "$volume" ] && [ -w "$volume" ] && [ -x "$volume" ]; then
    # Probe each existing component: a file, symlink, or read-only cache cannot
    # redirect builds elsewhere or be mistaken for a usable destination.
    usable=yes
    for directory in "$volume/$project" "$target"; do
        if [ -L "$directory" ]; then usable=no; break; fi
        if [ -e "$directory" ] && { [ ! -d "$directory" ] || [ ! -w "$directory" ] || [ ! -x "$directory" ]; }; then
            usable=no; break
        fi
    done
    if [ "$usable" = yes ]; then printf '%s\n' "$target"; exit 0; fi
fi
printf '%s\n' "$checkout/target"
