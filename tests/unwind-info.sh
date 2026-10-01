#!/bin/bash
source "$(dirname "$0")"/common.inc

# __unwind_info entries have no length: each covers the code up to the
# next. So ld-prime gives every atom of an instruction section an entry,
# and one without unwind information of its own gets encoding 0 ("no
# unwind info") instead of falling under the function before it.
echo 'int f(void) { return 1; }' | $CC -o $t/a.o -c -xc -
printf '.text\n.globl _g\n_g:\n ret\n.subsections_via_symbols\n' | $CC -o $t/b.o -c -xassembler -
echo 'int f(void); int main() { return f() - 1; }' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe

unwind_entries() {
  python3 - $1 <<'EOF2'
import struct, subprocess, sys
out = subprocess.run(['otool', '-l', sys.argv[1]], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __unwind_info':
        size = int(out[i + 3].split()[1], 16); off = int(out[i + 4].split()[1])
d = open(sys.argv[1], 'rb').read()[off:off + size]
_, ceo, cec, _, _, iso, isc = struct.unpack_from('<7I', d, 0)
common = struct.unpack_from(f'<{cec}I', d, ceo)
for k in range(isc - 1):
    first, page, _ = struct.unpack_from('<3I', d, iso + 12 * k)
    kind = struct.unpack_from('<I', d, page)[0]
    if kind == 3:
        _, eo, ec, eco, ecc = struct.unpack_from('<IHHHH', d, page)
        local = struct.unpack_from(f'<{ecc}I', d, page + eco)
        for e in struct.unpack_from(f'<{ec}I', d, page + eo):
            idx = e >> 24
            enc = common[idx] if idx < cec else local[idx - cec]
            print(hex(0x100000000 + first + (e & 0xffffff)), hex(enc))
    else:
        _, eo, ec = struct.unpack_from('<IHH', d, page)
        for j in range(ec):
            fo, enc = struct.unpack_from('<II', d, page + eo + 8 * j)
            print(hex(0x100000000 + fo), hex(enc))
EOF2
}
addr() { nm $1 | awk -v s=$2 '$3 == s { print "0x" $1 }' | sed 's/0x0*/0x/'; }
unwind_entries $t/exe > $t/entries
grep -q "^$(addr $t/exe _g) 0x0$" $t/entries

# For the same reason two consecutive functions with the same encoding
# share one entry, even with padding between them.
cat <<EOF | $CC -o $t/d.o -c -xc -
int h(void);
int f(void) { return h() + 1; }
__attribute__((aligned(64))) int main() { return f() - 3; }
EOF
echo 'int h(void) { return 2; }' | $CC -o $t/e.o -c -xc -
$CC --ld-path=$mold -o $t/exe2 $t/d.o $t/e.o
$t/exe2
unwind_entries $t/exe2 > $t/entries2
grep -q "^$(addr $t/exe2 _f) " $t/entries2
not grep -q "^$(addr $t/exe2 _main) " $t/entries2

# An empty atom gets an entry too: the empty __text of an object of
# only data without .subsections_via_symbols, which the arm64
# assembler labels (ltmp0), at the end of the code or ahead of the
# function that shares its address.
if [ $ARCH = arm64 ]; then
  printf '.data\n.quad 1\n' | $CC -o $t/f.o -c -xassembler -
  $CC --ld-path=$mold -o $t/exe4 $t/c.o $t/a.o $t/f.o
  $t/exe4
  unwind_entries $t/exe4 > $t/entries4
  [ "$(tail -1 $t/entries4 | cut -d' ' -f2)" = 0x0 ]
  $CC --ld-path=$mold -o $t/exe5 $t/f.o $t/c.o $t/a.o
  $t/exe5
  unwind_entries $t/exe5 > $t/entries5
  [ "$(sed -n 1p $t/entries5)" = "$(addr $t/exe5 _main) 0x0" ]
  [ "$(sed -n 2p $t/entries5 | cut -d' ' -f1)" = "$(addr $t/exe5 _main)" ]
fi

# Entries of encoding 0, code without unwind info, are never merged,
# whether a record or no record says so; code in a section that is not
# of pure instructions, which the assembler marks as holding some, is
# no function and gets none. The common encodings table ranks the
# encodings of the merged entries by use, ties in increasing order.
rec() { printf '.quad _%s\n.long 1\n.long %s\n.quad 0\n.quad 0\n' $1 $2; }
{
  echo .text
  for f in main z1 z2 bare a1 a2 a3 b1 c1 b2 c2; do
    printf '.globl _%s\n_%s:\n  ret\n' $f $f
  done
  printf '.section __TEXT,__bar,regular\n_in_bar:\n  ret\n'
  echo '.section __LD,__compact_unwind,regular,debug'
  echo '.p2align 3'
  rec main 0x02000000; rec z1 0; rec z2 0
  rec a1 0x02010000; rec a2 0x02010000; rec a3 0x02010000
  rec b1 0x02030000; rec c1 0x02020000; rec b2 0x02030000; rec c2 0x02020000
  echo .subsections_via_symbols
} | $CC -o $t/g.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe6 $t/g.o
unwind_entries $t/exe6 > $t/entries6
grep -q "^$(addr $t/exe6 _z2) 0x0$" $t/entries6
grep -q "^$(addr $t/exe6 _bare) 0x0$" $t/entries6
not grep -q "^$(addr $t/exe6 _a2) " $t/entries6
not grep -q "^$(addr $t/exe6 _in_bar) " $t/entries6
python3 - $t/exe6 > $t/common6 <<'EOF2'
import struct, subprocess, sys
out = subprocess.run(['otool', '-l', sys.argv[1]], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __unwind_info':
        off = int(out[i + 4].split()[1])
d = open(sys.argv[1], 'rb').read()[off:]
_, ceo, cec = struct.unpack_from('<3I', d, 0)
print(*[hex(e) for e in struct.unpack_from(f'<{cec}I', d, ceo)])
EOF2
[ "$(cat $t/common6)" = "0x0 0x2020000 0x2030000" ]

# With more entries than a page holds, ld-prime fills the 4096-byte
# second-level pages from the first function on, starts each page at
# an 8-byte boundary of the section, and sizes the first-level index
# for as many pages as the entries could take in the regular format
# (511 per page), plus the terminator and one spare, zero-filled.
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
python3 - $t/exe3 <<'EOF2'
import struct, subprocess, sys
out = subprocess.run(['otool', '-l', sys.argv[1]], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __unwind_info':
        size = int(out[i + 3].split()[1], 16); off = int(out[i + 4].split()[1])
d = open(sys.argv[1], 'rb').read()[off:off + size]
_, _, _, _, _, iso, isc = struct.unpack_from('<7I', d, 0)
pages = [struct.unpack_from('<3I', d, iso + 12 * k)[1] for k in range(isc - 1)]
counts = [struct.unpack_from('<IHH', d, p)[2] for p in pages]
lsda = struct.unpack_from('<3I', d, iso)[2]
assert len(counts) > 1, counts
assert all(c == counts[0] for c in counts[:-1]) and counts[-1] <= counts[0], counts
assert (lsda - iso) // 12 == -(-sum(counts) // 511) + 2, (lsda - iso, sum(counts))
assert all(p % 8 == 0 for p in pages[1:]), pages
assert len(d) % 8 == 0, len(d)
EOF2

# ld-prime orders the entries of one address by encoding, whatever the
# order of their records, so that the unwinder finds the greatest.
{
  printf '.text\n.globl _main\n_main:\n  ret\n'
  echo '.section __LD,__compact_unwind,regular,debug'
  echo '.p2align 3'
  rec main 0x02003000; rec main 0x02001000; rec main 0x02002000
  echo .subsections_via_symbols
} | $CC -o $t/h.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe7 $t/h.o
unwind_entries $t/exe7 > $t/entries7
[ "$(cut -d' ' -f2 $t/entries7 | tr '\n' ' ')" = '0x2001000 0x2002000 0x2003000 ' ]
