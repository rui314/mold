#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# An Objective-C class of a delay-init dylib can be used from macOS 15
# on: the class references fold into __got there, and code loads a class
# from its GOT entry, which the delay-init machinery routes through a
# load helper that dlopen()s the dylib - running its initializers -
# first. Below macOS 15 a class reference is an __objc_classrefs slot, a
# pointer dyld binds at launch and the runtime reads as it loads the
# image, which can't be delayed, so the link is refused.
cat <<EOF | $CC -o $t/k.o -c -xobjective-c - -mmacosx-version-min=14.0
#import <Foundation/Foundation.h>
#include <stdio.h>
@interface K : NSObject
+ (int)hello;
@end
@implementation K
+ (int)hello { return 42; }
@end
__attribute__((constructor)) static void init(void) { printf("k loaded\n"); }
EOF
$CC -o $t/libk.dylib -shared $t/k.o -framework Foundation -Wl,-install_name,@rpath/libk.dylib \
  -mmacosx-version-min=14.0

for v in 15.0 14.0; do
  cat <<EOF | $CC -o $t/main$v.o -c -xobjective-c - -mmacosx-version-min=$v
#import <Foundation/Foundation.h>
#include <stdio.h>
@interface K : NSObject
+ (int)hello;
@end
int main() {
  printf("start\n");
  printf("%d\n", [K hello]);
}
EOF
done

$CC --ld-path=$mold -o $t/exe $t/main15.0.o -Wl,-delay_library,$t/libk.dylib -Wl,-rpath,$PWD/$t \
  -framework Foundation -mmacosx-version-min=15.0
$RUN $t/exe > $t/out
printf 'start\nk loaded\n42\n' | cmp - $t/out

not $CC --ld-path=$mold -o $t/exe2 $t/main14.0.o -Wl,-delay_library,$t/libk.dylib \
  -Wl,-rpath,$PWD/$t -framework Foundation -mmacosx-version-min=14.0 2> $t/log
grep -q "K.* cannot be delayed" $t/log
