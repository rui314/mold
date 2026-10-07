#!/bin/bash
source "$(dirname "$0")"/common.inc

# -objc_stubs_small makes arm64's _objc_msgSend$<selector> stubs 12
# bytes, word-aligned: the selector load, then a branch to
# _objc_msgSend's __stubs entry, which shares its GOT slot with other
# references rather than taking one of its own. -objc_stubs_fast, the
# default, makes the usual 32-byte stubs, and the last of the two
# counts. ld-prime makes x86-64's stubs the same either way.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -fobjc-msgsend-selector-stubs -mmacosx-version-min=11.0 -
#import <Foundation/Foundation.h>
#import <objc/message.h>
@interface Foo : NSObject
- (int)alpha;
- (int)bravo;
@end
@implementation Foo
- (int)alpha { return 1; }
- (int)bravo { return 2; }
@end
void *addr(void) { return (void *)&objc_msgSend; }
int main() {
  Foo *f = [Foo new];
  printf("%d %d %d\n", [f alpha], [f bravo], addr() != 0);
}
EOF

sect() {
  otool -l $1 | grep -A6 "sectname $2\$" | awk '$1 == "size" || $1 == "align" { printf "%s ", $2 }'
}
got() {
  otool -Iv $1 | awk '/Indirect symbols for/ { s = /,__got\)/; next }
    s && $1 ~ /^0x/ { printf "%s ", $3 }'
}

for v in 11.0 13.0; do
  $CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=$v \
    -Wl,-objc_stubs_small
  $RUN $t/exe | grep -q '^1 2 1$'
  if [ $ARCH = arm64 ]; then
    [ "$(sect $t/exe __objc_stubs)" = '0x0000000000000018 2^2 ' ]
    [ "$(got $t/exe | grep -o '_objc_msgSend ' | tr -d '\n')" = '_objc_msgSend ' ]
    stub=$(otool -Iv $t/exe | awk '/Indirect symbols for/ { s = /,__stubs\)/; next }
      s && $3 == "_objc_msgSend" { sub(/^0x0*/, "0x", $1); print $1 }')
    objdump -d --no-show-raw-insn --section=__objc_stubs $t/exe > $t/dis
    [ "$(grep -Ec "	b	$stub( |\$)" $t/dis)" = 2 ]
  else
    [ "$(sect $t/exe __objc_stubs)" = '0x000000000000001a 2^5 ' ]
  fi

  $CC --ld-path=$mold -o $t/exe2 $t/a.o -framework Foundation -mmacosx-version-min=$v \
    -Wl,-objc_stubs_small -Wl,-objc_stubs_fast
  $RUN $t/exe2 | grep -q '^1 2 1$'
  if [ $ARCH = arm64 ]; then
    [ "$(sect $t/exe2 __objc_stubs)" = '0x0000000000000040 2^5 ' ]
    [ "$(got $t/exe2 | grep -o '_objc_msgSend ' | tr -d '\n')" = '_objc_msgSend ' ]
  fi
done

# The shared cache takes no small stubs, on either architecture.
not $CC --ld-path=$mold -o $t/c.dylib -shared $t/a.o -framework Foundation \
  -Wl,-install_name,/usr/lib/libc.dylib -Wl,-objc_stubs_small 2> $t/log
grep -q "Shared cache eligible dylibs cannot use '-objc_stubs_small'" $t/log
$CC --ld-path=$mold -o $t/c.dylib -shared $t/a.o -framework Foundation \
  -Wl,-install_name,/usr/lib/libc.dylib -Wl,-objc_stubs_small -Wl,-not_for_dyld_shared_cache
