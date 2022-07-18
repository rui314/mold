#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/NSObject.h>
@interface MyClass : NSObject
@end
@implementation MyClass
@end
EOF

ar rcs $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

clang -o $t/exe $t/c.o $t/b.a
! nm $t/exe | grep -q _OBJC_CLASS_ || false

! clang -o $t/exe $t/c.o $t/b.a -Wl,-ObjC > $t/log 2>&1
grep -q _OBJC_CLASS_ $t/log

echo OK
