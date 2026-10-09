#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -O2 -c -xc - -o $t/a.o
__attribute__((noinline)) static int only_a(int x) { return x * 3; }
int fa(int x) { return only_a(x) + 1; }
EOF
cat <<EOF | $CC -flto -O2 -c -xc - -o $t/b.o
int fb(int x) { return x + 2; }
EOF
echo 'int fn(int x) { return x; }' | $CC -O2 -c -xc - -o $t/n.o

# The map lists the object LTO compiled the bitcode files to among the
# objects, and credits it with the symbols it defines; a native
# object's stay its own. (ld-prime lists the bitcode files too, and
# credits each compiled symbol to the one that defined its name.)
$CC --ld-path=$mold -flto -dynamiclib -o $t/c.dylib $t/a.o $t/n.o $t/b.o -Wl,-map,$t/map
sed -n '/^# Object files:/,/^# Sections:/p' $t/map > $t/files
num() { sed -n "s|^\[ *\([0-9]*\)\] $1\$|\1|p" $t/files; }
n=$(num $t/n.o)
lto=$(num /tmp/lto.o)
[ -n "$n" ]
[ -n "$lto" ]
not grep -q -e "$t/a.o" -e "$t/b.o" $t/files

file_of() { grep "\] _$1\$" $t/map | sed 's/.*\[ *\([0-9]*\)\].*/\1/'; }
[ "$(file_of fa)" = $lto ]
[ "$(file_of fb)" = $lto ]
[ "$(file_of fn)" = $n ]
