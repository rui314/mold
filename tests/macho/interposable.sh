#!/bin/bash
source "$(dirname "$0")"/common.inc

# -interposable makes every export of an image interposable, and
# -interposable_list (which wins) those it names: the image calls them
# through stubs and binds its pointers to them to itself, so that a
# library interposing one (__DATA,__interpose) catches its own calls too.
cat <<EOF | $CC -o $t/a.o -c -xc - -O0
int foo(void) { return 1; }
int bar(void) { return 2; }
__attribute__((visibility("hidden"))) int hid(void) { return 3; }
int (*fp)(void) = foo;
int get(void) { return foo() * 100 + bar() * 10 + hid(); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int get(void);
int main() { printf("%d\n", get()); }
EOF

# An inserted library interposes foo.
cat <<EOF | $CC -o $t/c.o -c -xc -
int foo(void);
static int my_foo(void) { return 5; }
__attribute__((used, section("__DATA,__interpose"))) static struct {
  void *replacement, *replacee;
} interpose[] = {{(void *)my_foo, (void *)foo}};
EOF

$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o -Wl,-interposable
dyld_info -fixups $t/liba.dylib > $t/fixups
grep -Eq '__data .* bind +<this-image>/_foo$' $t/fixups
grep -Eq '__got .* bind +<this-image>/_bar$' $t/fixups
not grep -q _hid $t/fixups
otool -tV $t/liba.dylib > $t/text
grep -q 'symbol stub for: _foo' $t/text
not grep -q 'symbol stub for: _hid' $t/text
$CC --ld-path=$mold -o $t/exe $t/b.o $t/liba.dylib
$CC --ld-path=$mold -o $t/libc.dylib -shared $t/c.o $t/liba.dylib
run() {
  if on_simulator; then
    SIMCTL_CHILD_DYLD_INSERT_LIBRARIES=$t/libc.dylib $RUN $t/exe
  else
    DYLD_INSERT_LIBRARIES=$t/libc.dylib $t/exe
  fi
}
if native_arch; then
  run | grep -q '^523$'
fi

echo _foo > $t/list
$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o -Wl,-interposable \
  -Wl,-interposable_list,$t/list
dyld_info -fixups $t/liba.dylib > $t/fixups
grep -Eq 'bind +<this-image>/_foo$' $t/fixups
not grep -q _bar $t/fixups
if native_arch; then
  run | grep -q '^523$'
fi

# Without the option, the dylib calls its own foo.
$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o
if native_arch; then
  run | grep -q '^123$'
fi
