#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | $CC -o "$t"/a.o -c -xobjective-c -
#import <Foundation/NSObject.h>
@interface MyClass : NSObject
@end
@implementation MyClass
@end
EOF

ar rcs "$t"/b.a "$t"/a.o

cat <<EOF | $CC -o "$t"/c.o -c -xc -
int main() {}
EOF

clang -o "$t"/exe "$t"/c.o "$t"/b.a
! nm "$t"/exe | grep -q _OBJC_CLASS_ || false

! clang -o "$t"/exe "$t"/c.o "$t"/b.a -Wl,-ObjC > "$t"/log 2>&1
grep -q _OBJC_CLASS_ "$t"/log

echo OK
