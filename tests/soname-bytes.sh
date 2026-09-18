#!/usr/bin/env bash
. $(dirname $0)/common.inc

first=$'lib\xfe.so'
second=$'lib\xff.so'

echo 'V1 { foo; };' > $t/a.ver
echo 'V2 { bar; };' > $t/b.ver

# Use the system linker to produce the input SONAMEs independently of mold's
# command-line parser. The libraries also exercise .gnu.version_r output.
echo 'int foo() { return 42; }' |
  $CC -shared -fPIC -nostdlib -o "$t/$first" -xc - \
    -Wl,-soname,"$first",--version-script=$t/a.ver
echo 'int bar() { return 43; }' |
  $CC -shared -fPIC -nostdlib -o "$t/$second" -xc - \
    -Wl,-soname,"$second",--version-script=$t/b.ver
cp "$t/$first" $t/a.so
cp "$t/$second" $t/b.so

cat <<EOF | $CC -c -o $t/c.o -xc -
int foo();
int bar();
int main() { return foo() + bar() - 85; }
EOF

$CC -B. -o $t/exe $t/c.o $t/a.so $t/b.so -Wl,-rpath,"$PWD/$t"
$OBJCOPY --dump-section .dynstr=$t/dynstr $t/exe
grep -aqF "$first" $t/dynstr
grep -aqF "$second" $t/dynstr
$QEMU $t/exe
