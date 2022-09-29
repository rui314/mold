#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

# Skip if libc is musl
ldd --help 2>&1 | grep -q musl && { echo skipped; exit; }

# Skip if target is not x86-64
[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<'EOF' | $CC -c -o $t/a.o -x assembler -
.globl fn
fn:
  movabs main, %rax
  ret
EOF

cat <<EOF | $CC -c -o $t/b.o -fPIC -xc -
void fn();
int main() { fn(); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -pie -Wl,-warn-textrel >& $t/log
grep -q 'relocation against symbol `main'\'' in read-only section' $t/log
grep -q 'creating a DT_TEXTREL in an output file' $t/log

echo OK
