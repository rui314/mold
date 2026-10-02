#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib records an import its link names as an initial
# undefine (-u) though nothing refers to it, and an image that merges
# it, linked by either linker, imports it in its place.
cat <<EOF | $CC -o $t/a.o -c -xc -
int a(void) { return 7; }
EOF
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-u,_getpid \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib
nm -m $t/libfoo.dylib | grep -q '(undefined) external _getpid (from libSystem)'

cat <<EOF | $CC -o $t/main.o -c -xc -
int a(void);
int main() { return a() == 7 ? 0 : 1; }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
for exe in $t/exe $t/exe2; do
  $exe
  nm -m $exe > $exe.syms
  grep -q '(undefined) external _getpid (from libSystem)' $exe.syms
done
