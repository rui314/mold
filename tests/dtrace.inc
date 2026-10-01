# -*- mode: sh -*-
# Helpers for the dtrace-*.sh tests, sourced after common.inc.

# dtrace_header NAME: makes $t/NAME.h from the D script $t/NAME.d, with
# dtrace -h; a host without it skips the test.
dtrace_header() {
  command -v dtrace > /dev/null || skip
  dtrace -h -s $t/$1.d -o $t/$1.h >& /dev/null || skip
}

# dof_dump [-x] FILE: describes each DOF section (S_DTRACE_DOF) of an
# image, a line for the section and its provider, its attributes, and
# each probe with its argument types and the function of each instance,
# with the number of its probe sites and is-enabled tests. A site whose
# instruction in the image is not what the linker makes of one, a nop
# or a zeroing of the result register, is reported. -x adds the DOF's
# bytes in hex, each slot of a site's distance from the DOF zeroed.
dof_dump() {
  python3 - "$@" <<'EOF'
import struct, sys
hexdump = sys.argv[1] == '-x'
data = open(sys.argv[-1], 'rb').read()
u32 = lambda o: struct.unpack_from('<I', data, o)[0]
cputype, ncmds = u32(4), u32(16)
segs, sects, off = [], [], 32
for _ in range(ncmds):
    cmd, size = u32(off), u32(off + 4)
    if cmd == 0x19:
        vmaddr, vmsize, fileoff = struct.unpack_from('<QQQ', data, off + 24)
        segs.append((vmaddr, vmsize, fileoff))
        for i in range(u32(off + 64)):
            s = off + 72 + 80 * i
            name = data[s:s + 16].rstrip(b'\0').decode()
            addr, sz, foff, align = struct.unpack_from('<QQII', data, s + 32)
            sects.append((name, addr, sz, foff, align, u32(s + 64)))
    off += size
def at(addr, n):
    for vmaddr, vmsize, fileoff in segs:
        if vmaddr <= addr < vmaddr + vmsize:
            return data[fileoff + addr - vmaddr:][:n]
    return b''
if cputype == 0x0100000c:
    sites = (bytes.fromhex('1f2003d5'), bytes.fromhex('000080d2'))
    at_site = lambda a: at(a, 4)
else:
    sites = (bytes.fromhex('900f1f4000'), bytes.fromhex('33c0909090'))
    at_site = lambda a: at(a - 1, 5)
for name, addr, size, foff, align, flags in sects:
    if flags & 0xff != 0xf:
        continue
    dof = bytearray(data[foff:foff + size])
    secs = [struct.unpack_from('<IIIIQQ', dof, 64 + 32 * i) for i in range(u32(foff + 28))]
    by_type = {s[0]: s for s in secs}
    strtab = dof[by_type[8][4]:][:by_type[8][5]]
    string = lambda o: strtab[o:strtab.index(b'\0', o)].decode()
    prov = by_type[15][4]
    attrs = struct.unpack_from('<5I', dof, prov + 20)
    print(f'dof {name} {string(struct.unpack_from("<I", dof, prov + 16)[0])} flags 0x{flags:x} align {align}')
    print('attrs ' + ' '.join(f'0x{a:08x}' for a in attrs))
    pr = by_type[16]
    for k in range(pr[5] // 48):
        p = pr[4] + 48 * k
        func, pname, nargv = struct.unpack_from('<III', dof, p + 8)
        nargc, _, noffs = struct.unpack_from('<BBH', dof, p + 32)
        nenoffs = struct.unpack_from('<H', dof, p + 40)[0]
        args, o = [], nargv
        for _ in range(nargc):
            args.append(string(o))
            o += len(args[-1]) + 1
        print(f'probe {string(pname)}({", ".join(args)}) in {string(func)}: {noffs} sites, {nenoffs} tests')
    for ty, kind in ((18, 0), (26, 1)):
        if ty not in by_type:
            continue
        for k in range(by_type[ty][5] // 4):
            o = by_type[ty][4] + 4 * k
            site = addr + struct.unpack_from('<i', dof, o)[0]
            if at_site(site) != sites[kind]:
                print(f'bad site 0x{site:x}: {at_site(site).hex()}')
            dof[o:o + 4] = bytes(4)
    if hexdump:
        for i in range(0, len(dof), 32):
            print(dof[i:i + 32].hex())
EOF
}
