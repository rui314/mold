#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image bound for the dyld shared cache - linked with
# -add_split_seg_info, or a dylib installed in /usr/lib or
# /System/Library, unless -not_for_dyld_shared_cache - records its
# references between sections (LC_SEGMENT_SPLIT_INFO) and is laid out
# as ld-prime lays out such images: the selector references, the class
# data (with relative method lists), the lazy pointers and the GOT
# (with the slots of weak-lookup symbols, which ld-prime puts in a
# __weak_got of their own) join __DATA_CONST; and a dylib binds the
# pointers to its exported classes to itself.
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
grep -q '__DATA_CONST,__got ' $t/sects
grep -q '__DATA_CONST,__objc_selrefs ' $t/sects
grep -A10 'sectname __objc_selrefs' $t/lc | grep -q 'flags 0x00000000'
# The cache builder, not dyld, protects __DATA_CONST.
grep -A10 'segname __DATA_CONST' $t/lc | grep -q 'flags 0x0$'
dyld_info -fixups $t/a.dylib > $t/fixups
grep -q '__objc_classlist .* bind *<this-image>/_OBJC_CLASS_$_Foo' $t/fixups
grep -q '__got .* bind .*__ZZ6weakfnvE1s' $t/fixups

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

# With lazy binding the lazy pointers join __DATA_CONST too, and the
# indirect symbol table names their slots. (A simulator's objects are
# built for its version, which binds no stub lazily.)
if ! on_simulator; then
  link e.dylib -Wl,-add_split_seg_info -mmacosx-version-min=11.0
  sects e.dylib | grep -q '__DATA_CONST,__la_symbol_ptr '
  otool -Iv $t/e.dylib > $t/isyms_e
  grep -A4 '(__DATA_CONST,__la_symbol_ptr)' $t/isyms_e | grep _puts
fi

# The cache builder binds every symbol, so none may be looked up
# dynamically; an OS image should need no run paths, nor be found by
# one, and a dylib's static initializers slow down every process.
not link f.dylib -Wl,-add_split_seg_info -Wl,-U,_nothing 2> $t/log
grep -q "Shared cache eligible dylibs cannot use '-undefined dynamic_lookup' or '-U'" $t/log
link f.dylib -Wl,-add_split_seg_info -Wl,-install_name,@rpath/libf.dylib 2> $t/log
grep -q 'OS dylibs should not use @rpath for -install_name. Use absolute path instead' $t/log
link f.dylib -Wl,-install_name,@rpath/libf.dylib 2> $t/log
not grep -q 'should not use @rpath' $t/log

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

# A build for profiling, which an object's __llvm_prf_ section tells,
# has initializers by design: ld-prime checks none.
cat <<EOF | $CC -o $t/prf.o -c -xassembler -
.section __DATA,__llvm_prf_cnts
.quad 0
EOF
$CXX --ld-path=$mold -shared -o $t/g.dylib $t/g.o $t/prf.o -Wl,-add_split_seg_info 2> $t/log
not grep -q 'static initializer' $t/log
$CXX --ld-path=$mold -shared -o $t/g.dylib $t/g.o $t/prf.o -Wl,-no_inits
