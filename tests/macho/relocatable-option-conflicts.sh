#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime checks options against the kind of output. Only a main
# executable has a PIE flag, a main-thread stack, a __PAGEZERO or an
# entry point, so those options are errors in a -r link (-e only a
# warning) rather than silently ignored, and so is -data_const, whose
# split the final link decides. A dylib or bundle gets the same checks.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void) { return 1; }
EOF

r() { $mold -r -arch $ARCH -o $t/r.o $t/a.o "$@"; }

not r -pie >& $t/log1
grep -q -- '-pie can only be used when linking a main executable' $t/log1
not r -stack_size 0x4000 >& $t/log2
grep -q -- '-stack_size option can only be used when linking a main executable' $t/log2
not r -pagezero_size 0x4000 >& $t/log3
grep -q -- '-pagezero_size can only be used when linking a main executable' $t/log3
not r -client_name foo >& $t/log4
grep -q -- '-client_name can only be used when creating a bundle or main executable' $t/log4
not r -data_const >& $t/log5
grep -q -- '-data_const not supported with -r' $t/log5

r -e _foo >& $t/log6
grep -q 'warning: ignoring -e, not used for output type' $t/log6

# Nothing to say about the options that turn features off.
r -no_pie -no_data_const -pagezero_size 0 -image_base 0x4000 >& $t/log7
not grep -q warning $t/log7

so() { $CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o "$@"; }

not so -Wl,-stack_size,0x4000 >& $t/log8
grep -q -- '-stack_size option can only be used when linking a main executable' $t/log8
not so -Wl,-client_name,foo >& $t/log9
grep -q -- '-client_name can only be used when creating a bundle or main executable' $t/log9
so -Wl,-pie -Wl,-e,_foo -Wl,-pagezero_size,0 >& $t/log10
grep -q 'warning: -pie being ignored. It is only used when linking a main executable' $t/log10
grep -q 'warning: ignoring -e, not used for output type' $t/log10
