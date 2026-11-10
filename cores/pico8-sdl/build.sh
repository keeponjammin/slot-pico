#!/bin/sh
# Builds the 32-bit ARM SDL_Init shim PICO-8 runs with, see sdl_init.c.
# usage: build.sh OUT
# Uses $CC, else arm-linux-gnueabihf-gcc, else zig cc.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
out="$1"
mkdir -p "$(dirname "$out")"
if [ -n "${CC:-}" ]; then
	set -- $CC
elif command -v arm-linux-gnueabihf-gcc >/dev/null 2>&1; then
	set -- arm-linux-gnueabihf-gcc
elif command -v zig >/dev/null 2>&1; then
	set -- zig cc -target arm-linux-gnueabihf.2.28
else
	echo "build.sh: no 32-bit ARM compiler, install gcc-arm-linux-gnueabihf or zig" >&2
	exit 1
fi
"$@" -shared -fPIC -O2 -s -Wall -o "$out" "$here/sdl_init.c" -ldl
echo "built $out"
