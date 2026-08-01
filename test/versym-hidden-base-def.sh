#!/usr/bin/env bash
. $(dirname $0)/common.inc

# musl's dynamic linker ignores symbol versioning, so it may resolve the
# reference to either definition.
is_musl && skip

# A defined dynamic symbol at the base version (VER_NDX_GLOBAL) with the
# hidden bit set is a legacy compat alias. GNU as emits one for
# `.symver foo, foo@VER1` without the `remove` modifier so that binaries
# linked before the symbol was versioned keep resolving. Such a symbol must
# not satisfy a new unversioned reference to `foo`; that has to bind to the
# default version `foo@@VER3` instead.
#
# lksctp-tools' libsctp.so.1 ships sctp_connectx this way. Binding to the
# alias silently selects the old ABI, which never fills in the caller's
# association ID.

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo_v1() { return 1; }
int foo_v3() { return 3; }
__asm__(".symver foo_v1, foo@VER1");
__asm__(".symver foo_v3, foo@@VER3");
EOF

cat <<EOF > $t/a.ver
VER1 { global: foo; local: *; };
VER3 { global: foo; } VER1;
EOF

$CC -B. -shared -o $t/a.so $t/a.o -Wl,--version-script=$t/a.ver

# Rewrite foo@VER1's .gnu.version entry with VER_NDX_GLOBAL | VERSYM_HIDDEN.
# Current binutils drops the compat alias rather than emitting it, so patch
# the file with dd. st_name already points to plain "foo"; only the version
# index tells the two entries apart.
idx=$(readelf --dyn-syms -W $t/a.so |
      awk '$8 == "foo@VER1" { sub(/:/, "", $1); print $1 }')
off=$(readelf -SW $t/a.so | sed 's/\[ *[0-9]*\]//' |
      awk '$1 == ".gnu.version" { print $4 }')
[ -n "$idx" ] || skip
[ -n "$off" ] || skip

off=$((16#$off + idx * 2))
case $(od -A n -t x1 -j $off -N 2 $t/a.so | tr -d ' ') in
0280) printf '\x01\x80' | dd of=$t/a.so bs=1 seek=$off conv=notrunc status=none ;;
8002) printf '\x80\x01' | dd of=$t/a.so bs=1 seek=$off conv=notrunc status=none ;;
*) skip ;;
esac

cat <<EOF | $CC -c -o $t/b.o -xc -
int foo();
int main() { return foo() == 3 ? 0 : 1; }
EOF

$CC -B. -o $t/exe $t/b.o $t/a.so -Wl,-rpath,$t

# The reference must end up with a VER3 requirement. Without one the dynamic
# linker cannot reach foo@@VER3 at all: glibc rejects a non-base version for
# an unversioned lookup, so it would silently pick the compat alias.
readelf -V $t/exe | grep -q 'Name: VER3'
$QEMU $t/exe
