Lightweight partial Linux containers
====================================

Toolkit, and several examples of, non-system container environments on Linux.
Also, no dependency on system utilities (eg. `mount` or `ifconfig`).
(`newuidmap` and `newgidmap` are required only with non-privlaged user namespaces)

* `isolate <cmd> [args...]`

Run a command with most of the filesystem tree re-mounted as read-only,
with the exception of `$PWD`, also without network access.

May be installed with SUID set, or with non-privlaged user namespaces enabled.

* `hidehome <cmd> [args...]`

Run a command when `$PWD==$HOME` with `$HOME/..` hidden except for `$PWD`
(which must be under `$HOME`).
Parent directories will appear as empty except for the child leading to `$PWD`.

Intended to prevent (or at least complicate) misbehaved code from even
reading the contents of`$HOME`.

May be installed with SUID set, or with non-privlaged user namespaces enabled.

* `nonet <cmd> [args...]`

Run a command with no network access.  Only a loopback interface.

Should be installed with SUID set.

## Debug

```
export RUST_LOG=debug
```
