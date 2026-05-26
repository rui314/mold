#!/bin/bash
. $(dirname $0)/common.inc

# ICF folds two sections by hash/content equality only. If two read-only
# sections have identical bytes but different alignment requirements, mold
# must not fold them together: the follower would then resolve to the
# leader's address, which may not satisfy the follower's alignment, and
# any aligned-SIMD load against it would SIGSEGV.
#
# Use clang because mold relies on the .llvm_addrsig table that clang
# generates to decide ICF eligibility. The data must be address-insignifi-
# cant for ICF to consider folding, so the test files only access the
# constants by subscript and never take their address.

CLANG="${TEST_CLANG:-clang}"
$CLANG --version > /dev/null 2>&1 || skip

# Object 'a' is passed first, so its 4-byte-aligned constants get the
# lower priority and would be chosen as ICF leaders.
cat <<EOF | $CLANG -c -o $t/a.o -ffunction-sections -fdata-sections -fPIC -O2 -xc -
__attribute__((aligned(4))) const int x[4] = {1, 2, 3, 4};
int get_x(int i) { return x[i]; }
EOF

cat <<EOF | $CLANG -c -o $t/b.o -ffunction-sections -fdata-sections -fPIC -O2 -xc -
__attribute__((aligned(16))) const int x_aligned[4] = {1, 2, 3, 4};
int get_xa(int i) { return x_aligned[i]; }
EOF

./mold -shared -o $t/exe.so $t/a.o $t/b.o -icf=all \
    --print-icf-sections=$t/icf.log

# .rodata.x and .rodata.x_aligned have identical 16 bytes but different
# alignment requirements. They must not be folded together.
not grep -q x_aligned $t/icf.log
