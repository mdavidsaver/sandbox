Lightweight partial Linux containers
====================================

Toolkit, and several examples of, non-system container environments on Linux.
Also, no dependency on system utilities (eg. `mount` or `ifconfig`).
(`newuidmap` and `newgidmap` are required only with non-privlaged user namespaces)

* `hidehome <cmd> [args...]`

Run a command when `$PWD==$HOME` with `$HOME/..` hidden except for `$PWD`.
Prevent (or at least complicate) misbehaved code from trashing `$HOME`.

May be installed with SUID set, or with non-privlaged user namespaces enabled.

* `nonet <cmd> [args...]`

Run a command with no network access.  Only a loopback interface.

Should be installed with SUID set.
