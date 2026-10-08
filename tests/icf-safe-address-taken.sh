#!/usr/bin/env bash
. $(dirname $0)/common.inc

# See icf.sh.
[ $MACHINE = ppc64 ] && skip

# See icf-safe.sh.
if [ $MACHINE = s390x ]; then
  echo 'void *foo() { return foo; }' | $CC -c -o $t/a.o -xc -
  readelf -r $t/a.o | grep R_390_PLT32DBL && skip
fi

# See icf-string-literal.sh.
[[ $MACHINE = loongarch* ]] && skip

# foo1, foo2 and foo3 are identical, and the addresses of foo1 and foo2
# are taken. --icf=safe must not merge foo1 and foo2, but it can merge
# foo3 into one of them.
cat <<EOF | $CC -c -o $t/a.o -O2 -ffunction-sections -xc -
int foo1(int x) { return x * 7 + 3; }
int foo2(int x) { return x * 7 + 3; }
int foo3(int x) { return x * 7 + 3; }
EOF

# bar1 and bar2 differ only in which of foo1 and foo2 they return. Since
# foo1 and foo2 stay distinct, so must bar1 and bar2.
cat <<EOF | $CC -c -o $t/b.o -O2 -ffunction-sections -xc -
int foo1(int);
int foo2(int);
void *bar1(void) { return foo1; }
void *bar2(void) { return foo2; }
EOF

# Some targets call a function in non-PIC code by its absolute address,
# which makes the function address-taken. -fPIC avoids that.
cat <<EOF | $CC -c -o $t/c.o -fPIC -xc -
#include <stdio.h>

int foo1(int);
int foo2(int);
int foo3(int);
void *bar1(void);
void *bar2(void);

int main() {
  printf("%d %d %d\n", bar1() == bar2(), bar2() == foo2, foo3(1));
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o -Wl,--icf=safe,--print-icf-sections > $t/log
$QEMU $t/exe | grep '^0 1 10$'

grep -q 'selected section .*/a.o:(.text.foo1)' $t/log
grep -q 'removing identical section .*/a.o:(.text.foo3)' $t/log
not grep -q foo2 $t/log
