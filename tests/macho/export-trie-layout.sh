#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime lays the export trie out root first, then every other node
# in post-order - a node's subtrees in edge order, then the node - so a
# node's child offsets are known when it is written. The root is written
# before anything is placed, so it reserves 5 bytes (a u32's ULEB128)
# per child offset and leaves the unused ones zero. The trie is padded
# to 8 bytes.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo = 1, foobar = 2, fo = 3, zeta = 4, sym_a = 5, sym_b = 6, alpha = 7, al = 8;
int bar(void) { return 1; }
EOF
$CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl zfirst, afirst, mid
.data
zfirst: .quad 1
afirst: .quad 2
mid: .quad 3
EOF
$CC --ld-path=$mold -o $t/b.dylib -shared $t/b.o

layout() {
  python3 - $1 <<'EOF2'
import sys, macho
def uleb(b, i):
    v = s = 0
    while True:
        x = b[i]; i += 1; v |= (x & 0x7f) << s; s += 7
        if not x & 0x80: return v, i
b = macho.MachO(sys.argv[1]).linkedit_data(macho.LC_DYLD_EXPORTS_TRIE)
nodes = {}; stack = [(0, '')]
while stack:
    o, path = stack.pop()
    ts, i = uleb(b, o); i += ts
    n = b[i]; i += 1
    for _ in range(n):
        e = b.index(0, i); label = b[i:e].decode(); co, i = uleb(b, e + 1)
        stack.append((co, path + label))
    nodes[o] = (path, i)
order = sorted(nodes)
print(len(b) % 8, order[1] - nodes[0][1], ' '.join(nodes[o][0] or '-' for o in order))
EOF2
}

[ "$(layout $t/a.dylib)" = '0 4 - _alpha _al _bar _foobar _foo _fo _sym_a _sym_b _sym_ _zeta _' ]
[ "$(layout $t/b.dylib)" = '0 12 - afirst mid zfirst' ]
