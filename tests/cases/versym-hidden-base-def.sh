#!/usr/bin/env bash
. $(dirname $0)/common.inc

# musl's dynamic linker ignores symbol versions, so it may bind the
# reference to either definition.
is_musl && skip

# A shared library may contain a defined symbol whose version is
# VERSYM_HIDDEN | VER_NDX_GLOBAL. Such a symbol is a compatibility alias
# that exists only so that old binaries that were linked before the
# library adopted symbol versioning keep resolving. The linker must not
# bind a new reference to it; an unversioned reference has to bind to
# the default version `foo@@VER3` instead. lksctp-tools' libsctp.so.1
# exports sctp_connectx this way, and binding to the alias silently
# selects the old ABI.
#
# GNU as and ld create such an alias for `.symver foo_v0, foo@`, a
# symver directive with an empty version name. mold doesn't, so we need
# GNU ld to create a test input.
test_cflags -fuse-ld=bfd || skip

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo_v0() { return 0; }
int foo_v3() { return 3; }
__asm__(".symver foo_v0, foo@");
__asm__(".symver foo_v3, foo@@VER3");
EOF

echo 'VER3 { global: foo; local: *; };' > $t/a.ver
$CC -fuse-ld=bfd -shared -o $t/a.so $t/a.o -Wl,--version-script=$t/a.ver

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
int foo();
int main() { printf("%d\n", foo()); }
EOF

$CC -B. -o $t/exe $t/b.o $t/a.so -Wl,-rpath,$t
$QEMU $t/exe | grep 3
