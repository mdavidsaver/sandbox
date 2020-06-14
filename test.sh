#!/bin/sh
set -e

# simplify testing when testing when working directory is mounted nosuid

sudo install -m 04755 target/debug/hidehome /usr/local/bin/hidehome

/usr/local/bin/hidehome "$@"

sudo rm /usr/local/bin/hidehome
