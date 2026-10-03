#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib carries the subsections of the objects it was
# linked from (LC_ATOM_INFO), and an image that merges it gets them as
# it would the objects: code, data and its zero fill, tentative
# definitions, thread-local variables, initializers, weak definitions,
# literals and the compact unwind records, in the sections the objects
# would have them in. Their optimization hints aren't among them.
cat <<EOF | $CC -o $t/a.o -c -O1 -xc -
#include <stdio.h>
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
  printf("%s %s %d\n", msg, table[y % 3], sw(y));
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

# The main program shares no import with the library: ld-prime binds
# a merged import apart from the image's own, and so would list one
# twice.
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

$CC -shared -o $t/libfoo.dylib $t/a.o $t/b.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
otool -l $t/libfoo.dylib | grep -q LC_ATOM_INFO

mkdir -p $t/x $t/y
$CC --ld-path=$mold -o $t/x/exe $t/main.o -L$t -Wl,-merge-lfoo
$CC --ld-path=$mold -o $t/y/exe $t/main.o $t/a.o $t/b.o
$t/x/exe > $t/out
grep -q 'hello from a two 33' $t/out
grep -q '^16909092$' $t/out
$t/y/exe > $t/out1
cmp $t/out $t/out1
same_sections_and_symbols $t/x/exe $t/y/exe

otool -L $t/x/exe > $t/libs
not grep -q libfoo $t/libs

# -dead_strip takes what the merged code doesn't use away as ever.
$CC --ld-path=$mold -o $t/x/exe2 $t/main.o -L$t -Wl,-merge-lfoo -Wl,-dead_strip
$CC --ld-path=$mold -o $t/y/exe2 $t/main.o $t/a.o $t/b.o -Wl,-dead_strip
$t/x/exe2 > $t/out2
cmp $t/out $t/out2
same_sections_and_symbols $t/x/exe2 $t/y/exe2
