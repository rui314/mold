#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -O2 -c -xc - -o $t/a.o
__attribute__((noinline)) static int only_a(int x) { return x * 3; }
__attribute__((noinline)) static int both(int x) { return x + 1; }
int fa(int x) { return only_a(x) + both(x); }
EOF
cat <<EOF | $CC -flto -O2 -c -xc - -o $t/b.o
__attribute__((noinline)) static int both(int x) { return x + 2; }
int fb(int x) { return both(x); }
EOF
echo 'int fn(int x) { return x; }' | $CC -O2 -c -xc - -o $t/n.o

# The map lists each bitcode file where it was named, and the object
# LTO compiled them to last. Each symbol of that object is credited to
# the one bitcode file that defined a symbol of its name, statics too,
# or else to the object itself: two files' static "both" is ambiguous,
# and LTO renames one of them.
$CC --ld-path=$mold -flto -dynamiclib -o $t/c.dylib $t/a.o $t/n.o $t/b.o -Wl,-map,$t/map
sed -n '/^# Object files:/,/^# Sections:/p' $t/map > $t/files
num() { sed -n "s|^\[ *\([0-9]*\)\] $1\$|\1|p" $t/files; }
a=$(num $t/a.o)
n=$(num $t/n.o)
b=$(num $t/b.o)
lto=$(num /tmp/lto.o)
test "$a" -lt "$n"
test "$n" -lt "$b"
test "$b" -lt "$lto"
test "$(grep '^\[' $t/files | tail -1)" = "[$(printf '%3d' $lto)] /tmp/lto.o"

file_of() { grep "\] _$1\$" $t/map | sed 's/.*\[ *\([0-9]*\)\].*/\1/'; }
test "$(file_of fa)" = $a
test "$(file_of only_a)" = $a
test "$(file_of fb)" = $b
test "$(file_of fn)" = $n
test "$(file_of both)" = $lto

# An external symbol two files define weakly goes to the one whose
# definition won, the first, whichever copy LTO kept.
for f in d e; do
  echo "__attribute__((weak, noinline)) int shared(int x) { return x * 5; }
int f$f(int x) { return shared(x); }" | $CC -flto -O2 -c -xc - -o $t/$f.o
done
$CC --ld-path=$mold -flto -dynamiclib -o $t/d.dylib $t/e.o $t/d.o -Wl,-map,$t/map
sed -n '/^# Object files:/,/^# Sections:/p' $t/map > $t/files
test "$(file_of shared)" = $(num $t/e.o)
