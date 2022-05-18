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
cd "$(dirname "$0")"/../..
t=out/test/macho/$testname
mkdir -p $t

some_fw=$t/SF/SomeFramework.framework/
mkdir -p $some_fw

cat > $some_fw/SomeFramework.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/frameworks/SomeFramework.framework/SomeFramework'
current-version: 0000
compatibility-version: 150
reexported-libraries:
  - targets:         [ x86_64-macos, arm64-macos ]
    libraries:       [ ]
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _some_framework_print ]
    objc-classes:    [ SomeObjectiveC ]
    weak-symbols:    [ _weak_some_framework_print ]
...
EOF


cat <<EOF | clang -o $t/TestTBDFiles.o -c -xobjective-c -
#include <stdio.h>
#import <CoreFoundation/CoreFoundation.h>

// Interface Declaration for SomeFramework.framework

@interface SomeObjectiveC: NSObject

@end

void some_framework_print(char*);
void weak_some_framework_print(char*) __attribute__((weak));;

// End Interface Declaration for SomeFramework.framework

@interface TestTBDFiles: SomeObjectiveC
@end

@implementation TestTBDFiles

-(void) helloWorld {
  some_framework_print("Hello World");
  weak_some_framework_print("Hello World");
}

@end

int main(int argc, const char * argv[]) {
    @autoreleasepool {
        TestTBDFiles *tbd = [[TestTBDFiles alloc]init];
        [tbd helloWorld];
    }
    return 0;
}
EOF


clang --ld-path=./ld64 -F$t/SF/ -Wl,-framework,SomeFramework \
  -Wl,-framework,CoreFoundation -lobjc -o $t/exe $t/TestTBDFiles.o
otool -L $t/exe > $t/install_paths.log
grep -q '/usr/frameworks/SomeFramework.framework/SomeFramework' $t/install_paths.log

echo OK
