#!/bin/bash
source "$(dirname "$0")"/common.inc

# Among the definitions of a symbol that no live file defines, those of
# lazy archive members and dylib exports, a strong one beats a weak one
# whatever their order, and the order breaks only ties, as mold ranks
# them. ld-prime differs where a weak one comes first: it loads the
# first archive member that defines the symbol, weak or not, and that
# member beats a later dylib, and it ranks a dylib's weak export after
# any archive member's definition, even a weak one.
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int x(void);
int main() { printf("%d\n", x()); }
EOF
echo '__attribute__((weak)) int x(void) { return 1; }' | $CC -o $t/weak1.o -c -xc -
echo 'int x(void) { return 2; }' | $CC -o $t/strong.o -c -xc -
echo '__attribute__((weak)) int x(void) { return 3; }' | $CC -o $t/weak3.o -c -xc -

rm -f $t/both.a $t/weak1.a $t/strong.a $t/weak3.a
ar rcs $t/both.a $t/weak1.o $t/strong.o
ar rcs $t/weak1.a $t/weak1.o
ar rcs $t/strong.a $t/strong.o
ar rcs $t/weak3.a $t/weak3.o
$CC --ld-path=$mold -shared -o $t/libweak1.dylib $t/weak1.o
$CC --ld-path=$mold -shared -o $t/libstrong.dylib $t/strong.o
$CC --ld-path=$mold -shared -o $t/libweak3.dylib $t/weak3.o

link() {
  $CC --ld-path=$mold -o $t/exe $t/main.o "$@"
  $RUN $t/exe
}

# A dylib's weak export loses to a later dylib's strong one and to a
# later archive member's strong definition.
link $t/libweak1.dylib $t/libstrong.dylib | grep -q '^2$'
nm -m $t/exe | grep -q '(undefined) external _x (from libstrong)'
link $t/libweak1.dylib $t/strong.a | grep -q '^2$'
nm -m $t/exe | grep -q '(__TEXT,__text) external _x$'

# A strong definition first wins anyway, and of two weak exports the
# first does.
link $t/libstrong.dylib $t/weak1.a | grep -q '^2$'
link $t/strong.a $t/libweak1.dylib | grep -q '^2$'
link $t/libweak1.dylib $t/libweak3.dylib | grep -q '^1$'
link $t/weak3.a $t/weak1.a | grep -q '^3$'

if $mold -v 2>&1 | grep -q mold-macho; then
  # An archive's weak member that comes first loses to its strong one,
  # which alone is loaded, and to a later dylib's strong export
  # (ld-prime: 1).
  link $t/both.a | grep -q '^2$'
  $CC --ld-path=$mold -o $t/exe $t/main.o $t/both.a -Wl,-why_load > $t/log 2>&1
  grep -q "'_x' caused load of .*both.a(strong.o)" $t/log
  not grep -q weak1.o $t/log
  link $t/weak1.a $t/libstrong.dylib | grep -q '^2$'
  link $t/weak1.a $t/strong.a | grep -q '^2$'

  # Of a dylib's weak export and a later member's weak definition, the
  # first wins (ld-prime: 3).
  link $t/libweak1.dylib $t/weak3.a | grep -q '^1$'
fi
