#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
void foo() {}
EOF

cc --ld-path=./ld64 -shared -o $t/libfoo.dylib $t/a.o

cat <<EOF | cc -o $t/b.o -c -xc -
void bar() {}
EOF

cc --ld-path=./ld64 -shared -o $t/libbar.dylib $t/b.o -L$t -Wl,-reexport-lfoo

objdump --macho --dylibs-used $t/libbar.dylib | grep -q 'libfoo.*reexport'

cat <<EOF | cc -o $t/c.o -c -xc -
void baz() {}
EOF

cc --ld-path=./ld64 -shared -o $t/libbaz.dylib $t/c.o -L$t -Wl,-reexport-lbar

objdump --macho --dylibs-used $t/libbaz.dylib | grep -q 'libbar.*reexport'

cat <<EOF | cc -o $t/d.o -c -xc -
void foo();
void bar();
void baz();

int main() {
  foo();
  bar();
  baz();
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/d.o -L$t -lbaz

echo OK
