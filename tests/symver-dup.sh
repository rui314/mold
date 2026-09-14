#!/usr/bin/env bash
. $(dirname $0)/common.inc

# `foo@@VER` can also be referred to as `foo@VER`, so defining both is a
# duplicate definition. https://github.com/rui314/mold/issues/828

cat <<EOF > $t/a.ver
VERS_1.1 { global: foo; };
VERS_1.2 { global: foo; } VERS_1.1;
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
__asm__(".symver foo1,foo@VERS_1.2");
__asm__(".symver foo2,foo@@VERS_1.2");

void foo1() {}
void foo2() {}
EOF

! $CC -B. -shared -o $t/b.so $t/b.o -Wl,--version-script=$t/a.ver 2> $t/log
grep 'duplicate symbol: .*: foo@VERS_1.2' $t/log

# Defining different versions of the same symbol is fine.
cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
__asm__(".symver foo1,foo@VERS_1.1");
__asm__(".symver foo2,foo@@VERS_1.2");

void foo1() {}
void foo2() {}
EOF

$CC -B. -shared -o $t/c.so $t/c.o -Wl,--version-script=$t/a.ver
