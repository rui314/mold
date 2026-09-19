#!/bin/bash
source "$(dirname "$0")"/common.inc

# Two things ld64 keeps out of a final image's symbol table that we
# let through: a linker-private label (l...) that arrived as a private
# external, and a non-external local in the Objective-C list sections
# (clang's l_OBJC_LABEL_CLASS_$ in __objc_classlist, Swift's
# _objc_classes_* there) - while a demoted private external in those
# sections stays, as clang's __OBJC_LABEL_PROTOCOL_$_X does. And a -r
# output must carry .weak_def_can_be_hidden (N_WEAK_DEF with
# N_WEAK_REF) so that the final link can still auto-hide the
# definition, as ld-prime does for PLCrashReporter's template
# instantiations.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
@protocol Greeter
- (void)greet;
@end
@interface Foo : NSObject <Greeter>
@end
@implementation Foo
- (void)greet {}
@end
Protocol *proto(void) { return @protocol(Greeter); }
int main() { return 0; }
EOF2
nm -m $t/a.o | grep -q 'non-external l_OBJC_LABEL_CLASS_\$'
nm -m $t/a.o | grep -q 'private external \[no dead strip\] __OBJC_LABEL_PROTOCOL_\$_Greeter'
$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation
nm -m $t/exe > $t/nm
not grep -q 'OBJC_LABEL_CLASS' $t/nm
grep -q 'non-external (was a private external) __OBJC_LABEL_PROTOCOL_\$_Greeter' $t/nm
grep -q 'non-external (was a private external) __OBJC_PROTOCOL_REFERENCE_\$_Greeter' $t/nm
grep -q ' external _proto$' $t/nm

# (clang marks the instantiation can-be-hidden when optimizing.)
cat <<EOF2 | $CC -O2 -o $t/b.o -c -xc++ -
template <typename T> struct Box { T v; __attribute__((noinline)) T get() const { return v; } };
int use(Box<int> *b) { return b->get(); }
EOF2
cat <<EOF2 | $CC -O2 -o $t/c.o -c -xc++ -
template <typename T> struct Box { T v; __attribute__((noinline)) T get() const { return v; } };
int use(Box<int> *b);
int main() { Box<int> b = {3}; return use(&b) + b.get() - 6; }
EOF2
nm -m $t/b.o | grep -q 'weak external automatically hidden __ZNK3BoxIiE3getEv'
$mold -r -arch $ARCH -o $t/r.o $t/b.o $t/c.o
nm -m $t/r.o | grep -q 'weak external automatically hidden __ZNK3BoxIiE3getEv'
$CC --ld-path=$mold -o $t/exe2 $t/r.o
$t/exe2
nm $t/exe2 | grep -q ' t __ZNK3BoxIiE3getEv$'
