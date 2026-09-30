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
