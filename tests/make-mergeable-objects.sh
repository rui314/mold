#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib records the atoms of its objects (LC_ATOM_INFO) as
# ld-prime records them, and an image that merges it, linked by the
# linker or by ld-prime, gets them as it would the objects: code, data
# and its zero fill, tentative definitions, thread-local variables,
# initializers, weak definitions, literals, compact unwind records and
# absolute symbols. mold, which applies optimization hints as ld64
# did, is compared without them.
cat <<EOF | $CC -o $t/a.o -c -O1 -xc -
#include <stdio.h>
asm(".globl _abs_val\n_abs_val = 0x1234");
extern char abs_val;
int counter = 5;
static int hidden_counter;
int common_var;
__attribute__((visibility("hidden"))) int hid_fn(int x) { return x - 1; }
const char *msg = "hello from a";
static const char *table[] = { "zero", "one", "two" };
int foo(int x) { hidden_counter++; return x + counter; }
__attribute__((noinline)) int sw(int x) {
  switch (x) { case 0: return 11; case 1: return 22; case 2: return 33; case 3: return 44; default: return 0; }
}
double dbl(double d) { return d * 3.25 + 1.5; }
__attribute__((weak)) int weak_fn(void) { return 7; }
__attribute__((constructor)) static void init(void) { counter += 1; }
int bar(int y) {
  printf("%s %s %d %lx\n", msg, table[y % 3], sw(y), (long)&abs_val);
  return foo(y) + hid_fn(y) + weak_fn() + (int)dbl(1.0) + common_var;
}
EOF

cat <<EOF | $CC -o $t/b.o -c -O1 -xc -
extern int counter;
int bar(int);
static __thread int tlv = 3;
__thread int tlv_zero;
long long big[2] = { 0x1122334455667788LL, 0x0102030405060708LL };
int *ptr_into_big = (int *)&big[1] + 1;
int baz(int x) { tlv += x; tlv_zero++; return tlv + tlv_zero + bar(x) + counter + *ptr_into_big; }
EOF

cat <<EOF | $CC -o $t/main.o -c -O1 -xc -
#include <unistd.h>
int baz(int);
int main() {
  char buf[16];
  int n = baz(2), i = 15;
  buf[i] = '\n';
  do { buf[--i] = '0' + n % 10; n /= 10; } while (n);
  write(1, buf + i, 16 - i);
}
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o $t/b.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
otool -l $t/libfoo.dylib | grep -q LC_ATOM_INFO

nohints=
if $mold -v 2>&1 | grep -q mold-macho; then
  nohints=-Wl,-ignore_optimization_hints
fi

mkdir -p $t/x $t/y
$CC --ld-path=$mold -o $t/x/exe $t/main.o -L$t -Wl,-merge-lfoo $nohints
$CC --ld-path=$mold -o $t/y/exe $t/main.o $t/a.o $t/b.o $nohints
cmp $t/x/exe $t/y/exe
$t/x/exe > $t/out
grep -q 'hello from a two 33 1234' $t/out
grep -q '^16909092$' $t/out
otool -L $t/x/exe > $t/libs
not grep -q libfoo $t/libs

# ld-prime merges it as well.
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
$t/exe2 > $t/out2
cmp $t/out $t/out2
