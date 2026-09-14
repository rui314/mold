#!/usr/bin/env bash
. $(dirname $0)/common.inc

# mold reserves a large range of virtual address space for its internal
# arena but makes it accessible only as it is used, so that the reservation
# is not charged against the commit limit on systems with overcommit
# disabled. RLIMIT_DATA counts writable private mappings the same way.
# https://github.com/rui314/mold/issues/1644

# Sanitizer runtimes map far more writable memory than that.
( ulimit -d $((64 << 20)) && ./mold --version ) > /dev/null 2>&1 || skip

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

( ulimit -d $((4 << 20)) && $CC -B. -o $t/exe $t/a.o -Wl,--threads=1 )
$QEMU $t/exe
