#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -O2 -c -xc - -o $t/a.o
int fa1(int x) { return x + 1; }
int fa2(int x) { return x + 2; }
EOF
echo 'int fb(int x) { return x + 3; }' | $CC -flto -O2 -c -xc - -o $t/b.o
echo 'int fn(int x) { return x + 4; }' | $CC -O2 -c -xc - -o $t/n.o
first2() { nm -n $1 | grep ' T ' | head -2 | awk '{print $3}' | tr '\n' ' '; }

# An order file's file:symbol line names a symbol of the object LTO
# compiled by the bitcode file the symbol came from.
printf 'b.o:_fb\na.o:_fa2\n' > $t/order1
$CC --ld-path=$mold -flto -dynamiclib -o $t/c1.dylib $t/a.o $t/b.o $t/n.o \
  -Wl,-order_file,$t/order1
test "$(first2 $t/c1.dylib)" = '_fb _fa2 '

# -no_use_lto_filenames_in_order_file_matching takes the object's own
# name instead.
$CC --ld-path=$mold -flto -dynamiclib -o $t/c2.dylib $t/a.o $t/b.o $t/n.o \
  -Wl,-order_file,$t/order1 -Wl,-no_use_lto_filenames_in_order_file_matching
test "$(first2 $t/c2.dylib)" != '_fb _fa2 '

printf 'lto.o:_fb\nlto.o:_fa2\n' > $t/order2
$CC --ld-path=$mold -flto -dynamiclib -o $t/c3.dylib $t/a.o $t/b.o $t/n.o \
  -Wl,-order_file,$t/order2 -Wl,-no_use_lto_filenames_in_order_file_matching
test "$(first2 $t/c3.dylib)" = '_fb _fa2 '
