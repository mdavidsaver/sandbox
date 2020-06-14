#!/bin/sh
set -e

# simplify testing when testing when working directory is mounted nosuid

sudo install -m 04755 target/debug/nohome /usr/local/bin/nohome

/usr/local/bin/nohome "$@"

sudo rm /usr/local/bin/nohome
