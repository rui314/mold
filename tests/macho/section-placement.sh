#!/bin/bash
source "$(dirname "$0")"/common.inc

# Output sections go where ld64 puts them, with ld64's flags and in
# its order. Data that needs no writes after dyld's fixups moves from
# __DATA to __DATA_CONST (__const, __cfstring, the ObjC class/category/
# protocol lists, __mod_init_func ...); __StaticInit joins __text;
# literal pools join __TEXT,__const; the __LLVM segment and
# __objc_clsrolist are consumed. Linker-directing attributes
# (no_dead_strip, live_support, coalesced ...) are dropped from the
# output, except that the ObjC lists dyld scans stay no-dead-strip;
# __eh_frame carries its conventional flags. __stubs follows __text;
# __got leads __DATA_CONST; the ObjC runtime data precedes __data.
cat <<EOF2 | $CC -o $t/a.o -c -xobjective-c -fno-objc-arc -
#import <Foundation/Foundation.h>
@protocol Greeter
- (void)greet;
@end
@interface Foo : NSObject <Greeter>
@end
@implementation Foo
- (void)greet { NSLog(@"hi %@", [Foo class]); }
@end
@interface Foo (Extra)
- (void)extra;
@end
@implementation Foo (Extra)
- (void)extra {}
@end
void use(Foo *f) { [f greet]; [f extra]; }
Protocol *proto(void) { return @protocol(Greeter); }
void *const table[] = { (void *)&NSLog };
__attribute__((constructor)) static void init(void) {}
EOF2
cat <<EOF2 | $CC -o $t/b.o -c -xc++ -
struct S { S() { asm(""); } } s;
typedef float v4 __attribute__((vector_size(16)));
v4 vec(v4 x) { return x + (v4){1, 2, 3, 4}; }
int main() { return 0; }
EOF2
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __LLVM,__bitcode
.byte 1
.section __LLVM,__swift_modhash
.byte 2
.section __DATA,__objc_clsrolist,regular,no_dead_strip
.p2align 3
.quad 0
.section __DATA,__mine,regular,no_dead_strip
.p2align 3
_mine: .quad 3
.section __DATA,__coal,coalesced
.p2align 3
_coal: .quad 4
EOF2

# Classic dyld info (macOS 11) keeps __mod_init_func as a section.
# (Category merging is off so the category list stays to be placed.)
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o -framework Foundation \
  -mmacosx-version-min=11.0 -Wl,-no_objc_category_merging
otool -l $t/exe | awk '/^ *sectname/{s=$2} /^ *segname/{g=$2} /^ *flags/{if (s != "") print g","s, $2; s=""}' > $t/sects

for s in __const __cfstring __objc_classlist __objc_catlist __objc_protolist \
         __objc_imageinfo __mod_init_func; do
  grep -q "^__DATA_CONST,$s " $t/sects
  not grep -q "^__DATA,$s " $t/sects
done
# Protocol references are written by the runtime before macOS 15: they
# stay in __DATA, flags and all.
for s in __objc_selrefs __objc_classrefs __objc_protorefs __objc_const __objc_data __data __mine __coal; do
  grep -q "^__DATA,$s " $t/sects
done
grep -q '^__DATA,__objc_protorefs 0x1000000b$' $t/sects
not grep -q '__StaticInit\|__literal16\|__LLVM\|__objc_clsrolist' $t/sects
grep -q '^__TEXT,__const ' $t/sects

# Flags.
grep -q '^__DATA_CONST,__objc_classlist 0x10000000$' $t/sects
grep -q '^__DATA_CONST,__objc_catlist 0x10000000$' $t/sects
grep -q '^__DATA,__objc_selrefs 0x10000005$' $t/sects
grep -q '^__DATA_CONST,__objc_protolist 0x00000000$' $t/sects
not grep -q '__DATA_CONST,__objc_protorefs' $t/sects
grep -q '^__DATA,__mine 0x00000000$' $t/sects
grep -q '^__DATA,__coal 0x00000000$' $t/sects
grep -q '^__TEXT,__text 0x80000400$' $t/sects
if grep -q '__eh_frame' $t/sects; then   # arm64 usually has compact unwind only
  grep -q '^__TEXT,__eh_frame 0x6800000b$' $t/sects
fi
grep -q '^__DATA_CONST,__mod_init_func 0x00000009$' $t/sects

# Order.
awk -F'[ ,]' '{print $1","$2}' $t/sects > $t/order
python3 - $t/order <<'EOF2'
import sys
o = [l.strip() for l in open(sys.argv[1])]
def before(a, b): assert o.index(a) < o.index(b), (a, b, o)
before("__TEXT,__text", "__TEXT,__stubs")
before("__TEXT,__stubs", "__TEXT,__cstring")
before("__TEXT,__cstring", "__TEXT,__unwind_info")
if "__TEXT,__eh_frame" in o: before("__TEXT,__unwind_info", "__TEXT,__eh_frame")
before("__DATA_CONST,__got", "__DATA_CONST,__const")
before("__DATA_CONST,__const", "__DATA_CONST,__cfstring")
before("__DATA_CONST,__cfstring", "__DATA_CONST,__objc_classlist")
before("__DATA_CONST,__objc_classlist", "__DATA_CONST,__objc_catlist")
before("__DATA_CONST,__objc_catlist", "__DATA_CONST,__objc_protolist")
before("__DATA_CONST,__objc_protolist", "__DATA_CONST,__objc_imageinfo")
before("__DATA,__objc_selrefs", "__DATA,__objc_protorefs")
before("__DATA,__objc_const", "__DATA,__objc_selrefs")
before("__DATA,__objc_selrefs", "__DATA,__objc_classrefs")
before("__DATA,__objc_data", "__DATA,__data")
before("__DATA,__data", "__DATA,__mine")
EOF2
$t/exe

# From macOS 15 on, protocol references are dyld's to fix up and move
# to __DATA_CONST like the other lists.
$CC --ld-path=$mold -o $t/exe15 $t/a.o $t/b.o $t/c.o -framework Foundation \
  -mmacosx-version-min=15.0 -Wl,-no_objc_category_merging
otool -l $t/exe15 | awk '/^ *sectname/{s=$2} /^ *segname/{g=$2} /^ *flags/{if (s != "") print g","s, $2; s=""}' > $t/sects15
grep -q '^__DATA_CONST,__objc_protorefs 0x00000000$' $t/sects15
not grep -q '__DATA,__objc_protorefs' $t/sects15
grep -q '^__TEXT,__init_offsets 0x00000016$' $t/sects15
$t/exe15

# -no_data_const keeps everything in __DATA.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o $t/c.o -framework Foundation -Wl,-no_data_const \
  -Wl,-no_objc_category_merging
otool -l $t/exe2 | grep -q 'segname __DATA_CONST' && exit 1
otool -l $t/exe2 | grep -A1 'sectname __cfstring' | grep -q 'segname __DATA'
$t/exe2
