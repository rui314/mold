#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime lays out the _objc_msgSend$<selector> stubs sorted by
# selector, bytewise, and their selector references in the same order
# after the inputs' own. An input's selector reference to a stub's
# selector (the @selector(kilo) here) is taken over by the stub's: one
# slot per selector, in the stubs' order rather than at the input's.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fobjc-msgsend-selector-stubs -mmacosx-version-min=13.0 -
#import <Foundation/Foundation.h>
@interface Zz : NSObject
+ (int)kilo;
+ (int)alpha;
+ (int)zulu;
+ (int)bravo;
@end
@implementation Zz
+ (int)kilo { return 1; }
+ (int)alpha { return 2; }
+ (int)zulu { return 3; }
+ (int)bravo { return 4; }
@end
SEL sel_q(void) { return @selector(qqq); }
SEL sel_k(void) { return @selector(kilo); }
int main() {
  int v = [Zz kilo] * 1000 + [Zz alpha] * 100 + [Zz zulu] * 10 + [Zz bravo];
  printf("%d %d\n", v, [Zz respondsToSelector:sel_k()]);
}
EOF

$CC --ld-path=$mold -mmacosx-version-min=13.0 -o $t/exe $t/a.o -framework Foundation
$t/exe | grep -q '^1234 1$'

nm -n $t/exe | grep -o 'objc_msgSend\$.*' | tr '\n' ' ' > $t/stubs
[ "$(cat $t/stubs)" = 'objc_msgSend$alpha objc_msgSend$bravo objc_msgSend$kilo objc_msgSend$zulu ' ]

otool -ov $t/exe | sed -n '/__objc_selrefs) section/,/^Contents of/p' |
  awk 'NF == 2 { print $2 }' | tr '\n' ' ' > $t/selrefs
[ "$(cat $t/selrefs)" = 'qqq alpha bravo kilo zulu ' ]
