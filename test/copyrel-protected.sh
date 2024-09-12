#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = ppc64 ] && skip
[ $MACHINE = ppc64le ] && skip
[ $MACHINE = alpha ] && skip
[[ $MACHINE = loongarch* ]] && skip

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIE -
extern int foo;

int main() {
  return foo;
}
EOF

cat <<EOF | $CC -shared -o $t/b.so -xc -
__attribute__((visibility("protected"))) int foo;
EOF

! $CC -B. $t/a.o $t/b.so -o $t/exe >& $t/log -no-pie || false
grep -Fq 'cannot create a copy relocation for protected symbol' $t/log
