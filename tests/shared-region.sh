#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image bound for the dyld shared cache - linked with
# -add_split_seg_info, or a dylib installed in /usr/lib or
# /System/Library, unless -not_for_dyld_shared_cache - records its
# references between sections (LC_SEGMENT_SPLIT_INFO) and is laid out
# as ld-prime lays out such images: the selector references, the class
# data (with relative method lists), the lazy pointers and the GOT
# slots of weak-lookup symbols (__weak_got) join __DATA_CONST; the
# stubs and the Objective-C names follow __unwind_info; and a dylib
# binds the pointers to its exported classes to itself.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c++ -O1 -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
- (int)bar;
@end
@implementation Foo
- (int)bar { return 42; }
@end
inline int weakfn() { static int s; return ++s; }
extern "C" int use(Foo *f) { return [f bar] + weakfn() + (int)[Foo hash] + puts("x"); }
EOF

link() { $CXX --ld-path=$mold -shared -o $t/$1 $t/a.o -framework Foundation "${@:2}"; }
sects() { otool -l $t/$1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s { printf "%s,%s ", $2, s; s = "" }'; }

link a.dylib -Wl,-add_split_seg_info
otool -l $t/a.dylib > $t/lc
grep -q 'cmd LC_SEGMENT_SPLIT_INFO$' $t/lc
sects a.dylib > $t/sects
grep -q '__DATA_CONST,__objc_const ' $t/sects
grep -q '__DATA_CONST,__weak_got ' $t/sects
grep -q '__DATA_CONST,__objc_selrefs ' $t/sects
grep -q '__TEXT,__unwind_info .*__TEXT,__stubs .*__TEXT,__objc_methname __TEXT,__objc_methtype ' $t/sects
grep -A10 'sectname __objc_selrefs' $t/lc | grep -q 'flags 0x00000000'
# The cache builder, not dyld, protects __DATA_CONST.
grep -A10 'segname __DATA_CONST' $t/lc | grep -q 'flags 0x0$'
dyld_info -fixups $t/a.dylib > $t/fixups
grep -q '__objc_classlist .* bind *<this-image>/_OBJC_CLASS_$_Foo' $t/fixups
grep -q '__weak_got .* bind .*__ZZ6weakfnvE1s' $t/fixups

# Without, or opting out, none of that.
link b.dylib
sects b.dylib > $t/sects_b
otool -l $t/b.dylib | grep -A10 'segname __DATA_CONST' | grep -q 'flags 0x10$'
grep -q '__DATA,__objc_const ' $t/sects_b
not grep -q '__weak_got' $t/sects_b
link c.dylib -Wl,-add_split_seg_info -Wl,-not_for_dyld_shared_cache
otool -l $t/c.dylib > $t/lc_c
not grep -q 'cmd LC_SEGMENT_SPLIT_INFO$' $t/lc_c

# The install name alone makes an OS dylib eligible.
link d.dylib -Wl,-install_name,/System/Library/Frameworks/Foo.framework/Foo
otool -l $t/d.dylib > $t/lc_d
grep -q 'cmd LC_SEGMENT_SPLIT_INFO$' $t/lc_d

# With lazy binding the lazy pointers lead __DATA_CONST, and the
# indirect symbol table lists the sections in the image's order.
link e.dylib -Wl,-add_split_seg_info -mmacosx-version-min=11.0
[ "$(sects e.dylib | grep -o '__DATA_CONST,[^ ]*' | head -1)" = __DATA_CONST,__la_symbol_ptr ]
r1() { otool -l $t/e.dylib | awk -v s=$1 '$1 == "sectname" { n = $2 } $1 == "reserved1" && n == s { print $2 }'; }
[ "$(r1 __stubs)" -lt "$(r1 __la_symbol_ptr)" ]
[ "$(r1 __la_symbol_ptr)" -lt "$(r1 __weak_got)" ]
[ "$(r1 __weak_got)" -lt "$(r1 __got)" ]

# The cache builder binds every symbol, so none may be looked up
# dynamically; an OS image should need no run paths, and a dylib's
# static initializers slow down every process.
not link f.dylib -Wl,-add_split_seg_info -Wl,-U,_nothing 2> $t/log
grep -q "Shared cache eligible dylibs cannot use '-undefined dynamic_lookup' or '-U'" $t/log

cat <<EOF | $CXX -o $t/g.o -c -xc++ -
#include <unistd.h>
int x = getpid();
EOF
$CXX --ld-path=$mold -shared -o $t/g.dylib $t/g.o -Wl,-add_split_seg_info -Wl,-rpath,/x 2> $t/log
grep -q 'OS dylibs should not add rpaths' $t/log
grep -q "static initializer '__GLOBAL__sub_I_.*' found in '.*g.o'. Use -no_inits" $t/log
$CXX --ld-path=$mold -shared -o $t/g.dylib $t/g.o -Wl,-add_split_seg_info -Wl,-no_warn_inits 2> $t/log
not grep -q 'static initializer' $t/log
not $CXX --ld-path=$mold -shared -o $t/g.dylib $t/g.o -Wl,-no_inits 2> $t/log
grep -q 'Static initializers:' $t/log
