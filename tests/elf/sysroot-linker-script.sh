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

# The library search can find a script staged as a symlink into another tree.
mkdir -p $t/sysroot/foo/bar
ar rcs $t/sysroot/foo/bar/libfromsysroot.a $t/a.o
cat <<EOF > $t/backing.script
INPUT(/foo/bar/libfromsysroot.a)
EOF
ln -sf ../../../backing.script $t/sysroot/foo/bar/libscript.so
$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$t/sysroot \
  -L$t/sysroot/foo/bar -lscript

abs=$(realpath $t)
$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$abs/sysroot \
  -L$abs/sysroot/foo/bar -lscript

# A script found outside the sysroot can still name an absolute host path.
cat <<EOF > $t/outside.script
INPUT($abs/foo/bar/libfoo.a)
EOF
ln -sf outside.script $t/liboutside.so
$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$abs/sysroot -L$abs -loutside

# The same script found inside the sysroot cannot fall back to the host.
ln -sf ../../../outside.script $t/sysroot/foo/bar/libinside.so
not $CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$abs/sysroot \
  -L$abs/sysroot/foo/bar -linside |&
  grep -F "cannot open $abs/sysroot$abs/foo/bar/libfoo.a"

# A relative script name can use either its parent's directory or -L.
printf 'INPUT(libscript.so)\n' > $t/sysroot/foo/bar/outer.script
$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$t/sysroot \
  $t/sysroot/foo/bar/outer.script

printf 'INPUT(libscript.so)\n' > $t/search.script
$CC -B. -o $t/exe $t/c.o -Wl,--sysroot=$t/sysroot \
  -L$t/sysroot/foo/bar $t/search.script
