#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
int main() {
  NSProcessInfo *info = [NSProcessInfo processInfo];
  NSLog(@"processName: %@", [info processName]);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -framework foundation -Wl,-ObjC
$t/exe 2>&1 | grep -Fq 'processName: exe'

echo OK
