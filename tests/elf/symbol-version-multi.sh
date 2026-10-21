#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Old assemblers cannot give one symbol more than one version.
cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC || skip
void foo() {}
__asm__(".symver foo, foo@TEST1");
__asm__(".symver foo, foo@@TEST2");
EOF

cat <<EOF > $t/b.version
TEST1 { local: *; };
TEST2 {};
TEST3 {};
EOF

$CC -B. -o $t/c.so -shared $t/a.o -Wl,--version-script=$t/b.version

readelf -W --dyn-syms $t/c.so | grep -F 'foo@TEST1'
readelf -W --dyn-syms $t/c.so | grep -F 'foo@@TEST2'

# The directives in the other order put the versions in the other order
# in the symbol table.
cat <<EOF | $CC -o $t/d.o -c -xc - -fPIC
void foo() {}
__asm__(".symver foo, foo@@TEST2");
__asm__(".symver foo, foo@TEST1");
EOF

$CC -B. -o $t/e.so -shared $t/d.o -Wl,--version-script=$t/b.version

readelf -W --dyn-syms $t/e.so | grep -F 'foo@TEST1'
readelf -W --dyn-syms $t/e.so | grep -F 'foo@@TEST2'

# A version script can give the default version instead.
cat <<EOF | $CC -o $t/f.o -c -xc - -fPIC
void foo() {}
__asm__(".symver foo, foo@TEST1");
EOF

echo 'TEST1 {}; TEST2 { foo; };' > $t/g.version
$CC -B. -o $t/h.so -shared $t/f.o -Wl,--version-script=$t/g.version

readelf -W --dyn-syms $t/h.so | grep -F 'foo@TEST1'
readelf -W --dyn-syms $t/h.so | grep -F 'foo@@TEST2'
