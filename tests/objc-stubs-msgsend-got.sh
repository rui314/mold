#!/bin/bash
source "$(dirname "$0")"/common.inc

# The objc_msgSend$<selector> stubs load _objc_msgSend from a __got
# slot of their own. When code elsewhere also needs a slot for it (here
# its address, taken through the GOT), ld-prime gives that one a second
# slot, right after the stubs', and binds both.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -fobjc-msgsend-selector-stubs -
#import <Foundation/Foundation.h>
#import <objc/message.h>
#import <stdio.h>
@interface Foo : NSObject
- (int)greet;
@end
@implementation Foo
- (int)greet { return 1; }
@end
void *addr(void) { return (void *)&objc_msgSend; }
int main() { Foo *f = [Foo new]; printf("%d %d\n", [f greet], addr() != 0); return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -fno-objc-arc -fobjc-msgsend-selector-stubs -
#import <Foundation/Foundation.h>
#import <stdio.h>
@interface Foo : NSObject
- (int)greet;
@end
@implementation Foo
- (int)greet { return 1; }
@end
int main() { Foo *f = [Foo new]; printf("%d\n", [f greet]); return 0; }
EOF

got() {
  otool -Iv $1 | awk '/Indirect symbols for/ { s = /,__got\)/; next }
    s && $1 ~ /^0x/ { printf "%s ", $3 }'
}

if [ $ARCH = arm64 ]; then classic=11.0; else classic=12.0; fi
for v in $classic 15.0; do
  $CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=$v
  $t/exe | grep -q '^1 1$'
  [ "$(got $t/exe | grep -o '_objc_msgSend ' | tr -d '\n')" = '_objc_msgSend _objc_msgSend ' ]
  dyld_info -fixups $t/exe > $t/fixups
  [ "$(grep -c '__got .*bind .*/_objc_msgSend' $t/fixups)" = 2 ]

  $CC --ld-path=$mold -o $t/exe2 $t/b.o -framework Foundation -mmacosx-version-min=$v
  $t/exe2 | grep -q '^1$'
  [ "$(got $t/exe2 | grep -o '_objc_msgSend ' | tr -d '\n')" = '_objc_msgSend ' ]
done
