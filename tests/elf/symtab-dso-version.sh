#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Like GNU ld, we name a versioned DSO symbol `name@version` in .symtab.
cat <<EOF > $t/a.ver
VER1 { foo; };
EOF

cat <<EOF | $CC -fPIC -shared -o $t/a.so -Wl,--version-script=$t/a.ver -xc -
void foo() {}
void bar() {}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo();
void bar();
int main() { foo(); bar(); }
EOF

$CC -B. -o $t/exe $t/b.o $t/a.so
nm $t/exe > $t/log
grep -E 'U foo@VER1$' $t/log
grep -E 'U bar$' $t/log
