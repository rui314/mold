#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo();
int main() { foo(); }
EOF

# A subframework of "Big" that also admits the client "friend".
$CC --ld-path=$mold -shared -o $t/libsub.dylib $t/a.o \
  -Wl,-umbrella,Big -Wl,-allowable_client,friend
otool -l $t/libsub.dylib | grep -q 'client friend'

# A random client is rejected.
not $CC --ld-path=$mold -o $t/exe $t/b.o $t/libsub.dylib 2> $t/log
grep -q 'not an allowed client' $t/log

# The named client and the umbrella itself may link it.
$CC --ld-path=$mold -o $t/exe $t/b.o $t/libsub.dylib -Wl,-client_name,friend
$CC --ld-path=$mold -o $t/exe $t/b.o $t/libsub.dylib -Wl,-client_name,Big

# So may a sibling subframework of the same umbrella.
$CC --ld-path=$mold -shared -o $t/libsib.dylib $t/a.o \
  -Wl,-umbrella,Big $t/libsub.dylib

# The derived client name comes from the output leaf, minus "lib".
$CC --ld-path=$mold -o $t/friend $t/b.o $t/libsub.dylib
