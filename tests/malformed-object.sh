#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime checks an object's layout before it reads it, in this order,
# and refuses it at the first fault, naming the file twice: each load
# command in turn (numbered from 0) - that it lies within the load
# commands and is of a size a multiple of 4, then what it holds -, then
# the load commands an object may have one of only, its platforms, its
# segment and sections, and last the tables at the end of the file. An
# archive member whose load commands it refuses is no object to it,
# which it passes over without a word.

# Writes a copy OUT of the object IN with one fault MUT (see below), and
# prints the number of the load command it changed, if any.
cat > $t/mut.py <<'EOF'
import struct, sys
src, dst, mut = sys.argv[1:4]
d = bytearray(open(src, 'rb').read())
u32 = lambda off: struct.unpack_from('<I', d, off)[0]
set32 = lambda off, v: struct.pack_into('<I', d, off, v & 0xffffffff)
ncmds, sizeofcmds = u32(16), u32(20)
cmds, off = [], 32
for i in range(ncmds):
    cmds.append((i, off, u32(off), u32(off + 4)))
    off += u32(off + 4)
def find(kind):
    return next(c for c in cmds if c[2] == kind)
sects = [find(0x19)[1] + 72 + 80 * k for k in range(u32(find(0x19)[1] + 64))]
symtab, last = find(0x2), cmds[-1]
def as_last(cmd, size):
    set32(last[1], cmd); set32(last[1] + 4, size); set32(20, sizeofcmds - last[3] + size)
if mut == 'ncmds':
    set32(16, ncmds + 1)
elif mut == 'not-pointer-sized':
    set32(last[1] + 4, last[3] + 1)
elif mut == 'too-small':
    set32(last[1] + 4, 4)
elif mut == 'too-large':
    set32(last[1] + 4, last[3] + 8)
elif mut == 'symtab-size':
    set32(symtab[1] + 4, 32); print(symtab[0])
elif mut == 'too-few-strings':
    # The linker option, last, is "-lz" and padding.
    for k in range(last[1] + 12, last[1] + last[3]):
        d[k] = d[k] or ord('x')
    print(last[0])
elif mut == 'load-dylib':
    as_last(0xc, last[3]); print(last[0])
elif mut == 'rpath':
    as_last(0x8000001c, last[3]); print(last[0])
elif mut == 'data-in-code-size':
    as_last(0x29, 24); print(last[0])
elif mut == 'two-symtabs':
    as_last(0x2, 24); d[last[1] + 8:last[1] + 24] = d[symtab[1] + 8:symtab[1] + 24]
elif mut == 'ios':
    as_last(0x25, 16)
elif mut == 'segment-past-file':
    set32(find(0x19)[1] + 40, len(d))
elif mut == 'section-past-segment':
    struct.pack_into('<Q', d, sects[0] + 40, 0x100000)
elif mut == 'overlap':
    set32(symtab[1] + 12, u32(symtab[1] + 12) + 1)
elif mut == 'n_sect':
    d[u32(symtab[1] + 8) + 5] = 0x7f
elif mut == 'misplaced':
    struct.pack_into('<Q', d, u32(symtab[1] + 8) + 8, 0x100000)
open(dst, 'wb').write(d)
EOF
mut() { python3 $t/mut.py $t/a.o $t/$1.o $1; }

cat <<'EOF' | $CC -o $t/a.o -c -xc -
__asm__(".linker_option \"-lz\"");
int x = 1;
int main() { return x; }
EOF

link() { not $CC --ld-path=$mold -o $t/exe $t/$1.o 2> $t/$1.log; }
refused() { grep -v '^+' $t/$1.log | grep -Fq "$2 in '$t/$1.o' in '$t/$1.o'"; }

mut ncmds
link ncmds
grep -Eq "malformed load command \([0-9]+ of [0-9]+\) at offset=0x[0-9A-F]+ with mh=0x[0-9a-f]+, off end of load commands in '$t/ncmds.o' in '$t/ncmds.o'" $t/ncmds.log
mut not-pointer-sized
link not-pointer-sized
grep -q 'cmdsize=0x11 is not pointer sized' $t/not-pointer-sized.log
mut too-small
link too-small
grep -q 'size (0x4) too small' $t/too-small.log
mut too-large
link too-large
grep -q 'size (0x18) is too large, load commands end at offset 0x' $t/too-large.log

n=$(mut symtab-size)
link symtab-size
refused symtab-size "load command #$n LC_SYMTAB size wrong"
n=$(mut too-few-strings)
link too-few-strings
refused too-few-strings "load command #$n has too few strings (0) expected (1)"
n=$(mut load-dylib)
link load-dylib
refused load-dylib "load command #$n LC_LOAD_DYLIB not supported in .o files"
n=$(mut rpath)
$CC --ld-path=$mold -o $t/exe $t/rpath.o 2> $t/rpath.log
grep -q "warning: load command #$n LC_RPATH not supported in .o files" $t/rpath.log

mut two-symtabs
link two-symtabs
refused two-symtabs 'multiple LC_SYMTAB load commands found'
mut ios
link ios
grep -Eq "incompatible platforms: macOS - iOS(-simulator)? in '$t/ios.o'" $t/ios.log
mut segment-past-file
link segment-past-file
grep -v '^+' $t/segment-past-file.log |
  grep -Eq "segment '' content \(fileOffset: 0x[0-9A-F]+ -> 0x[0-9A-F]+\) extends beyond end of file \(length: 0x[0-9A-F]+\) in '$t/segment-past-file.o'"
mut section-past-segment
link section-past-segment
grep -Eq "section '__text' end address 0x[0-9A-F]+ is beyond containing segment's end address 0x[0-9A-F]+" $t/section-past-segment.log
mut overlap
link overlap
refused overlap 'LINKEDIT overlap of symbol table and symbol strings'
n=$(mut data-in-code-size)
link data-in-code-size
refused data-in-code-size "load command #$n LC_?? size is wrong"

# An archive member whose load commands ld-prime refuses is none of the
# link's; one it refuses past them, an object all the same.
echo 'int y = 2;' | $CC -o $t/y.o -c -xc -
rm -f $t/libbad.a $t/libbad2.a
cp $t/ncmds.o $t/m1.o
cp $t/overlap.o $t/m2.o
ar rcs $t/libbad.a $t/m1.o 2> /dev/null
ar rcs $t/libbad2.a $t/m2.o 2> /dev/null
$CC --ld-path=$mold -shared -o $t/b.dylib $t/y.o -Wl,-force_load,$t/libbad.a 2> $t/member.log
not grep -q 'malformed' $t/member.log
not $CC --ld-path=$mold -shared -o $t/b.dylib $t/y.o -Wl,-force_load,$t/libbad2.a 2> $t/member2.log
grep -q "LINKEDIT overlap of symbol table and symbol strings in '$t/libbad2.a(m2.o)'" $t/member2.log


# Whatever is cut off it, mold refuses an object without crashing.
size=$(wc -c < $t/a.o)
for n in 1 31 40 100 200 300 400 500 $((size - 8)); do
  head -c $n $t/a.o > $t/cut.o
  $CC --ld-path=$mold -o $t/exe $t/cut.o > $t/cut.log 2>&1 && continue
  not grep -q panicked $t/cut.log
done
