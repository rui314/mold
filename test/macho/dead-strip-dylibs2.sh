#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh $0)
mkdir -p $t

mkdir -p $t/Foo.framework

cat <<EOF | cc -o $t/Foo.framework/Foo -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | cc -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

cd $t

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-F. -Wl,-framework,Foo
otool -l $t/exe | grep -A3 'cmd LC_LOAD_DYLIB' | fgrep -q Foo.framework/Foo

clang -fuse-ld=$mold -o $t/exe $t/a.o -Wl,-F. -Wl,-framework,Foo \
  -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 'cmd LC_LOAD_DYLIB' >& $t/log
! fgrep -q Foo.framework/Foo $t/log || false

echo OK
