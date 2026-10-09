#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The code LTO compiles may call runtime routines no input called, such
# as memset for a loop that fills an array. As mold does, the linker
# resolves the symbols again after LTO, which loads the archive member
# that defines such a routine then, in a -static image and one dyld
# loads alike, while a member that defines a routine nothing comes to
# call stays out. A routine whose member is bitcode too is undefined
# then, unless -u has it loaded before LTO and compiled with the rest.
# (ld-prime "softloads" memset, __udivdi3 and six other routines before
# LTO, in a -static or -preload image or with
# -lto_softload_runtime_symbols, loading the members that define them
# whether or not anything comes to call them.)
lto_library=$(dirname "$(xcrun -f clang)")/../lib/libLTO.dylib

cat <<EOF | $CC -flto -O2 -c -xc - -o $t/fill.o
char buf[4096];
int fill(int n) {
  for (int i = 0; i < n; i++)
    buf[i] = 1;
  return buf[7];
}
EOF
echo 'int f(void) { return 0; }' | $CC -flto -O2 -c -xc - -o $t/f.o
cat <<EOF > $t/ms.c
void *memset(void *p, int c, unsigned long n) {
  char *q = p;
  while (n--)
    *q++ = c;
  return p;
}
int from_ms(void) { return 42; }
EOF
$CC -O2 -fno-builtin -c $t/ms.c -o $t/ms.o
$CC -O2 -fno-builtin -flto -c $t/ms.c -o $t/ms-bc.o
rm -f $t/libms.a $t/libms-bc.a
ar rcs $t/libms.a $t/ms.o
ar rcs $t/libms-bc.a $t/ms-bc.o

static() {
  $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -static \
    -lto_library $lto_library "$@"
}

# LTO's code calls memset: the member that defines it loads.
static -e _fill -o $t/static $t/fill.o $t/libms.a -map $t/map
grep -q 'libms.a(ms.o)$' $t/map

# Nothing calls it: the member stays out.
static -e _f -o $t/static2 $t/f.o $t/libms.a -map $t/map2
if is_mold; then
  not grep -q 'libms.a(ms.o)$' $t/map2
fi
$CC --ld-path=$mold -flto -dynamiclib -o $t/libf.dylib $t/f.o $t/libms.a \
  -Wl,-lto_softload_runtime_symbols
nm $t/libf.dylib > $t/nm-libf
if is_mold; then
  not grep -q _from_ms $t/nm-libf
fi

# The routine's member is bitcode.
if is_mold; then
  not static -e _fill -o $t/static3 $t/fill.o $t/libms-bc.a 2> $t/log3
  grep -q 'undefined symbol.*_memset' $t/log3
fi
static -e _fill -o $t/static4 $t/fill.o $t/libms-bc.a -u _memset
nm $t/static4 > $t/nm4
grep -q ' _memset$' $t/nm4

# A program's LTO code calls the memset of an archive named before
# libSystem.
cat <<EOF | $CC -O2 -fno-builtin -c -xc - -o $t/ms-print.o
#include <unistd.h>
void *memset(void *p, int c, unsigned long n) {
  char *q = p;
  write(1, "memset\n", 7);
  while (n--)
    *q++ = c;
  return p;
}
EOF
rm -f $t/libms-print.a
ar rcs $t/libms-print.a $t/ms-print.o
cat <<EOF | $CC -flto -O2 -c -xc - -o $t/main.o
#include <stdio.h>
char buf[4096];
int main(int argc, char **argv) {
  for (int i = 0; i < argc + 100; i++)
    buf[i] = 1;
  printf("%d\n", buf[7]);
}
EOF
$CC --ld-path=$mold -flto -o $t/exe $t/main.o $t/libms-print.a
$RUN $t/exe > $t/out
grep -q '^memset$' $t/out
grep -q '^1$' $t/out
