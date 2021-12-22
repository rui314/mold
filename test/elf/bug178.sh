#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

# Verify that mold does not crash if no object file is included
# in the output. The resulting executable doesn't contain any
# meaningful code or data, so this is an edge case, though.

cat <<EOF | cc -x assembler -c -o $t/a.o -
.globl foo
foo:
EOF

rm -f $t/a.a
ar rcs $t/a.a $t/a.o

$mold -o $t/exe $t/a.a

echo OK
