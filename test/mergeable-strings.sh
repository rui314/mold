#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -O2
#include <uchar.h>
#include <wchar.h>

char *cstr1 = "foo";
wchar_t *wide1 = L"foo";
char16_t *utf16_1 = u"foo";
char32_t *utf32_1 = U"foo";
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -O2
#include <stdio.h>
#include <uchar.h>
#include <wchar.h>

extern char *cstr1;
extern wchar_t *wide1;
extern char16_t *utf16_1;
extern char32_t *utf32_1;

char *cstr2 = "foo";
wchar_t *wide2 = L"foo";
char16_t *utf16_2 = u"foo";
char32_t *utf32_2 = U"foo";

int main() {
  printf("%p %p %p %p %p %p %p %p\n",
         cstr1, cstr2, wide1, wide2, utf16_1, utf16_2, utf32_1, utf32_2);
}
EOF

# String merging is an optional feature, so test it with the default
# linker first to verify that it does work on this system.
$CC -o $t/exe1 $t/a.o $t/b.o -no-pie

if $QEMU $t/exe1 | grep -E '^(\S+) \1 (\S+) \2 (\S+) \3 (\S+) \4$'; then
  $CC -B. -o $t/exe2 $t/a.o $t/b.o -no-pie
  $QEMU $t/exe2 | grep -E '^(\S+) \1 (\S+) \2 (\S+) \3 (\S+) \4$'
fi
