#!/bin/sh
set -e

# simplify testing when testing when working directory is mounted nosuid

name="$1"
shift

sudo install -m 04755 target/debug/"$name" /usr/local/bin/"$name"

/usr/local/bin/"$name" "$@"

sudo rm /usr/local/bin/"$name"
