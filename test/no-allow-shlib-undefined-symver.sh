#!/bin/bash
. $(dirname $0)/common.inc

[ "$(uname)" = FreeBSD ] && skip

# libfoo.so defines foo@VER_1.0 as a non-default version. Referencing
# the bare name `foo` does NOT bind to this symbol.
cat <<EOF > $t/libfoo.ver
VER_1.0 {};
EOF

cat <<EOF | $CC -B. -shared -fPIC -o $t/libfoo.so -xc - \
  -Wl,--version-script=$t/libfoo.ver
__asm__(".symver foo_impl,foo@VER_1.0");
int foo_impl(void) { return 42; }
EOF

# libbar.so references foo@VER_1.0 via .symver. Its dynsym holds an
# undefined symbol with name "foo" whose versym indexes into the
# verneed (.gnu.version_r) entry naming "VER_1.0".
cat <<EOF | $CC -B. -shared -fPIC -o $t/libbar.so -xc - -L$t -lfoo
__asm__(".symver foo_v1,foo@VER_1.0");
int foo_v1(void);
int call_foo_v1(void) { return foo_v1(); }
EOF

cat <<EOF | $CC -c -o $t/a.o -xc -
int call_foo_v1(void);
int main(void) { return call_foo_v1(); }
EOF

# Both libs are present; libbar.so's undef foo@VER_1.0 is satisfied by
# libfoo.so. --no-allow-shlib-undefined must accept the link.
$CC -B. -o $t/exe $t/a.o -Wl,--no-allow-shlib-undefined -L$t -lbar -lfoo
