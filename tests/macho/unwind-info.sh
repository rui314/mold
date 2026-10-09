#!/bin/bash
source "$(dirname "$0")"/common.inc

# __unwind_info entries have no length: each covers the code up to the
# next. So every subsection of an instruction section gets an entry,
# and one without unwind information of its own gets encoding 0 ("no
# unwind info") instead of falling under the function before it.
echo 'int f(void) { return 1; }' | $CC -o $t/a.o -c -xc -
printf '.text\n.globl _g\n_g:\n ret\n.subsections_via_symbols\n' | $CC -o $t/b.o -c -xassembler -
echo 'int f(void); int main() { return f() - 1; }' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$RUN $t/exe
unwind_lookup $t/exe _f _g _main > $t/enc
[ "$(sed -n 2p $t/enc)" = 0x0 ]
not grep -q '^0x0$' <(sed -n '1p;3p' $t/enc)

# Two consecutive functions with the same encoding may share one entry,
# even with padding between them; each still finds its encoding.
cat <<EOF | $CC -o $t/d.o -c -xc -
int h(void);
int f(void) { return h() + 1; }
__attribute__((aligned(64))) int main() { return f() - 3; }
EOF
echo 'int h(void) { return 2; }' | $CC -o $t/e.o -c -xc -
$CC --ld-path=$mold -o $t/exe2 $t/d.o $t/e.o
$RUN $t/exe2
unwind_lookup $t/exe2 _f _main _h > $t/enc2
[ "$(sed -n 1p $t/enc2)" = "$(sed -n 2p $t/enc2)" ]
not grep -q '^0x0$\|none' $t/enc2

# An empty subsection gets an entry too: the empty __text of an object
# of only data without .subsections_via_symbols, which the arm64
# assembler labels (ltmp0), at the end of the code or ahead of the
# function that shares its address - which still finds its own.
if [ $ARCH = arm64 ]; then
  printf '.data\n.quad 1\n' | $CC -o $t/f.o -c -xassembler -
  $CC --ld-path=$mold -o $t/exe4 $t/c.o $t/a.o $t/f.o
  $RUN $t/exe4
  [ "$(unwind_lookup $t/exe4 _f)" = "$(unwind_lookup $t/exe _f)" ]
  $CC --ld-path=$mold -o $t/exe5 $t/f.o $t/c.o $t/a.o
  $RUN $t/exe5
  [ "$(unwind_lookup $t/exe5 _main)" = "$(unwind_lookup $t/exe _main)" ]
fi

# Code a record says has no unwind info (encoding 0) and code with no
# record both find encoding 0; code in a section that is not of pure
# instructions, which the assembler marks as holding some, is no
# function and gets no entry of its own.
rec() { printf '.quad _%s\n.long 1\n.long %s\n.quad 0\n.quad 0\n' $1 $2; }
{
  echo .text
  for f in main z1 z2 bare a1 a2 a3 b1 c1 b2 c2; do
    printf '.globl _%s\n_%s:\n  ret\n' $f $f
  done
  printf '.section __TEXT,__bar,regular\n.globl _in_bar\n_in_bar:\n  ret\n'
  echo '.section __LD,__compact_unwind,regular,debug'
  echo '.p2align 3'
  rec main 0x02000000; rec z1 0; rec z2 0
  rec a1 0x02010000; rec a2 0x02010000; rec a3 0x02010000
  rec b1 0x02030000; rec c1 0x02020000; rec b2 0x02030000; rec c2 0x02020000
  echo .subsections_via_symbols
} | $CC -o $t/g.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe6 $t/g.o
unwind_lookup $t/exe6 _main _z1 _z2 _bare _a1 _a2 _a3 _b1 _c1 _b2 _c2 | tr '\n' ' ' > $t/enc6
[ "$(cat $t/enc6)" = '0x2000000 0x0 0x0 0x0 0x2010000 0x2010000 0x2010000 0x2030000 0x2020000 0x2030000 0x2020000 ' ]
objdump --unwind-info $t/exe6 > $t/unwind6
not grep -qi "function offset=0x0*$(nm $t/exe6 | awk '$3 == "_in_bar" { print $1 }' | sed 's/^0*1000//')," $t/unwind6

# More entries than a page holds: the lookups go through the
# first-level index to the right page.
python3 - > $t/many.c <<'EOF2'
print('int printf(const char *, ...);')
for i in range(2500):
    if i % 2:
        print(f'int f{i}(int x) {{ return x + {i}; }}')
    else:
        print(f'int f{i}(int x) {{ return printf("%d", x + {i}) + 1; }}')
print('int main() { return f1(-1); }')
EOF2
$CC -O1 -momit-leaf-frame-pointer -o $t/many.o -c $t/many.c
$CC --ld-path=$mold -o $t/exe3 $t/many.o
$RUN $t/exe3
unwind_lookup $t/exe3 $(for i in $(seq 0 2499); do echo _f$i; done) > $t/enc3
# The leaf functions share a mode (x86-64 describes each by an FDE of
# its own), the others an encoding. (clang gives an x86_64 simulator's
# object an FDE only for a function compact unwind can't describe.)
if [ $ARCH = arm64 ] || ! on_simulator; then
  python3 - $t/enc3 <<'EOF2'
import sys
enc = [int(l, 16) for l in open(sys.argv[1])]
modes = [e & 0x0f000000 for e in enc]
assert 0 not in enc
assert len(set(enc[0::2])) == 1 and len(set(modes[1::2])) == 1 and modes[0] != modes[1]
EOF2
fi
objdump --unwind-info $t/exe3 > $t/unwind3
[ "$(grep -c 'Second level index\[' $t/unwind3)" -gt 1 ]

# Of several records for one function, the unwinder finds the one of
# the greatest encoding, whatever their order.
{
  printf '.text\n.globl _main\n_main:\n  ret\n'
  echo '.section __LD,__compact_unwind,regular,debug'
  echo '.p2align 3'
  rec main 0x02003000; rec main 0x02001000; rec main 0x02002000
  echo .subsections_via_symbols
} | $CC -o $t/h.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe7 $t/h.o
[ "$(unwind_lookup $t/exe7 _main)" = 0x2003000 ]
