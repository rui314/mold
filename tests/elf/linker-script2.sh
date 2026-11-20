#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

mkdir -p $t/foo/bar
rm -f $t/foo/bar/libfoo.a
ar rcs $t/foo/bar/libfoo.a $t/a.o

cat <<EOF > $t/b.script
INPUT(-lfoo)
EOF

$CC -B. -o $t/exe -L$t/foo/bar $t/b.script

# Keep the searched path in DT_NEEDED for a script's -l of a DSO with no SONAME.
cat <<EOF | $CC -B. -fPIC -shared -o $t/foo/bar/libscript_dso.so -xc -
int script_dso(void) { return 0; }
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
int script_dso(void);
int main() { return script_dso(); }
EOF

cat <<EOF > $t/dso.script
INPUT(-lscript_dso)
EOF

$CC -B. -o $t/exe2 $t/c.o -L$t/foo/bar $t/dso.script
readelf --dynamic $t/exe2 | grep -F "Shared library: [$t/foo/bar/libscript_dso.so]"

$CC -B. -o $t/exe3 $t/c.o -L$t/foo/bar -lscript_dso
readelf --dynamic $t/exe3 | grep -F 'Shared library: [libscript_dso.so]'
