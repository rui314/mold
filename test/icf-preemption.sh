#!/bin/bash
. $(dirname $0)/common.inc

# On PPC64V1, function pointers refer function descriptors in .opd
# instead of directly referring .text section.
[ $MACHINE = ppc64 ] && skip

cat <<EOF | $CC -c -o $t/a.o -ffunction-sections -fPIC -xc -
void local_fn(void) {}
__attribute__((visibility("hidden"))) void local_fn_hidden(void) {
  local_fn();
}

void global_fn(void) {}

void caller_local(void) {
  local_fn_hidden();
}

void caller_global(void) {
  global_fn();
}
EOF

# Link as shared library with --icf=all
$CC -B. -shared -o $t/libtest.so $t/a.o -Wl,--icf=all -Wl,--print-icf-sections > $t/out 2>&1

# Verify that caller_local and caller_global were NOT folded.
# If they were folded, one of them would be removed.
not grep -q "removing identical section.*caller_local" $t/out
not grep -q "removing identical section.*caller_global" $t/out

# Also check with nm to be sure they have different addresses
nm $t/libtest.so | grep -E 'caller_local|caller_global' | awk '{print $1}' | uniq | wc -l | grep -q 2
