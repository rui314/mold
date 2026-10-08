#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = armeb ] && skip
[[ $MACHINE = loongarch* ]] && skip

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

mkdir -p $t/foo/bar
rm -f $t/foo/bar/libfoo.a
ar rcs $t/foo/bar/libfoo.a $t/a.o

cat <<EOF > $t/foo/bar/b.script
INPUT(/foo/bar/libfoo.a)
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$t/ $t/foo/bar/b.script

# Without --sysroot, an absolute path is not searched for in library paths.
not ./mold -o $t/exe -L$t $t/c.o $t/foo/bar/b.script |& grep 'cannot open /foo/bar/libfoo.a'
