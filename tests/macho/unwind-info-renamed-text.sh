#!/bin/bash
source "$(dirname "$0")"/common.inc

# __unwind_info stays in __TEXT when -rename_section or -rename_segment
# moves the code out, and covers the functions at their addresses in
# the new segment, which are known only once every segment is placed.
cat <<EOF | $CXX -o $t/a.o -c -xc++ -O1 -
#include <stdio.h>
__attribute__((noinline)) int thrower(int x) { if (x > 0) throw x; return x; }
int main(int argc, char **) {
  try { thrower(argc); } catch (int x) { printf("caught %d\n", x); }
}
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o -Wl,-rename_section,__TEXT,__text,__TEXT_EXEC,__text
otool -l $t/exe | grep -q 'sectname __unwind_info'
$RUN $t/exe | grep -q '^caught 1$'

$CXX --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-rename_segment,__TEXT,__FOO
otool -l $t/exe2 | grep -A1 'sectname __unwind_info' | grep -q 'segname __TEXT'
