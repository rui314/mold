#!/bin/bash
source "$(dirname "$0")"/common.inc

# Of the labels of a fixed-size literal, ld-prime's -map names the
# atom by the one best to name an atom - a global before a local, then
# the later name - unless that one is linker-private (lCPI0_0), which
# leaves the literal known by its size; each other label follows the
# atom's row with no size, the better first.
cat <<'EOF' | $CC -o $t/a.o -c -xc -
__asm__(".section __TEXT,__literal8,8byte_literals\n"
        "ka:\nkb:\n.quad 42\n"
        "lc1:\nlc2:\nlc3:\n.quad 43\n"
        "kd:\nlc4:\n.quad 44\n"
        ".globl _kg\n_kg:\nke:\n.quad 45\n");
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
grep -E '\] (k[a-z]|_kg|lc[0-9]|8-byte-literal)$' $t/map | cut -f2- > $t/rows
diff - $t/rows <<EOF
0x00000008	[  1] kb
0x00000000	[  1] ka
0x00000008	[  1] 8-byte-literal
0x00000000	[  1] lc2
0x00000000	[  1] lc1
0x00000008	[  1] 8-byte-literal
0x00000000	[  1] kd
0x00000008	[  1] _kg
0x00000000	[  1] ke
EOF
