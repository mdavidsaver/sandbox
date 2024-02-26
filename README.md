Lightweight partial Linux containers
====================================

Toolkit, and several examples of, non-system container environments on Linux.
Also, no dependency on system utilities (eg. `mount` or `ifconfig`) except
`newuidmap` and `newgidmap` which are required only with non-privlaged user namespaces.

* `isolate [options] <cmd> [args...]`

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

## Building

```sh
git clone https://github.com/mdavidsaver/sandbox
cd sandbox
```

```sh
cargo test
cargo build
```

Or for fully static executables.
Suggested when installing with SUID.

```sh
cargo test --release --target x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

Install

```sh
sudo install -m 04775 \
  target/x86_64-unknown-linux-musl/release/{hidehome,nonet,isolate} \
  /usr/local/bin/
```

### Building on Debian

Building with [packaged dependencies on Debian](https://wiki.debian.org/Rust)

```sh
sudo apt-get -y install cargo \
    librust-libc+rustc-dep-of-std-dev \
    librust-log-dev \
    librust-signal-hook-dev \
    librust-bindgen-dev

git clone https://github.com/mdavidsaver/sandbox
cd sandbox

mkdir .cargo
cat <<EOF > .cargo/config
# see https://wiki.debian.org/Rust
[source]
[source.debian-packages]
directory = "/usr/share/cargo/registry"
[source.crates-io]
replace-with = "debian-packages"
EOF

cargo test
```

## Debug

```
export RUST_LOG=DEBUG
```
