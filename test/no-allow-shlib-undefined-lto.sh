#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -flto || skip

# Test that --no-allow-shlib-undefined does not falsely report an error
# when LTO internalizes a symbol that a DSO also provides.
#
# When a static LTO object defines a symbol with hidden visibility,
# it wins initial resolution over the DSO definition. Then
# compute_import_export skips it (hidden), so the LTO plugin sees
# LDPR_PREVAILING_DEF_IRONLY and discards it. After re-resolution,
# the DSO definition should satisfy the other DSO's undefined reference.

cat <<EOF | $CC -B. -shared -fPIC -o $t/libfoo.so -xc -
void foo() {}
EOF

cat <<EOF | $CC -B. -shared -fPIC -o $t/libbar.so -xc -
void foo();
void bar() { foo(); }
EOF

cat <<EOF | $CC -flto -fvisibility=hidden -c -o $t/a.o -xc -
void foo() {}
void bar();
int main() { bar(); }
EOF

$CC -B. -o $t/exe $t/a.o -flto -Wl,--no-allow-shlib-undefined -L$t -lbar -lfoo
