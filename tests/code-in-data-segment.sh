#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section of pure instructions in __DATA puts code outside __TEXT,
# which __unwind_info covers too: the section is encoded again once
# every segment is placed, and if it grew the layout is done again -
# with LC_MAIN sized before the entry point has its address, while
# __TEXT keeps the one the first round gave it.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__foo,regular,pure_instructions
.long 1
EOF
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$RUN $t/exe
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o
$RUN $t/exe2
