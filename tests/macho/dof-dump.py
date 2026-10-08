#!/usr/bin/env python3
"""Describes each DOF section (S_DTRACE_DOF) of a Mach-O image.

For each one it prints a line for the section and its provider, its
attributes, and each probe with its argument types and the function of
each instance, with the number of its probe sites and is-enabled tests.
A site whose instruction in the image is not what the linker makes of
one, a nop or a zeroing of the result register, is reported.

Usage: dof-dump.py IMAGE
"""

import struct
import sys

LC_SEGMENT_64 = 0x19
S_DTRACE_DOF = 0xF
CPU_TYPE_ARM64 = 0x0100000C

DOF_SECT_STRTAB = 8
DOF_SECT_PROVIDER = 15
DOF_SECT_PROBES = 16
DOF_SECT_PROFFS = 18
DOF_SECT_PRENOFFS = 26


class Image:
    def __init__(self, data):
        self.data = data
        self.segments = []  # (vmaddr, vmsize, fileoff)
        self.sections = []  # (name, addr, size, fileoff, align, flags)
        ncmds = self.u32(16)
        off = 32
        for _ in range(ncmds):
            cmd, size = self.u32(off), self.u32(off + 4)
            if cmd == LC_SEGMENT_64:
                vmaddr, vmsize, fileoff = struct.unpack_from('<QQQ', data, off + 24)
                self.segments.append((vmaddr, vmsize, fileoff))
                for i in range(self.u32(off + 64)):
                    s = off + 72 + 80 * i
                    name = data[s:s + 16].rstrip(b'\0').decode()
                    addr, sz, foff, align = struct.unpack_from('<QQII', data, s + 32)
                    self.sections.append((name, addr, sz, foff, align, self.u32(s + 64)))
            off += size

    def u32(self, off):
        return struct.unpack_from('<I', self.data, off)[0]

    def at(self, addr, n):
        """The n bytes at a virtual address, or none if it is unmapped."""
        for vmaddr, vmsize, fileoff in self.segments:
            if vmaddr <= addr < vmaddr + vmsize:
                return self.data[fileoff + addr - vmaddr:][:n]
        return b''


def main():
    image = Image(open(sys.argv[1], 'rb').read())

    # What the linker writes at a probe site and at an is-enabled test,
    # and the bytes of a site to compare with them.
    if image.u32(4) == CPU_TYPE_ARM64:
        # nop; mov x0, #0
        sites = (bytes.fromhex('1f2003d5'), bytes.fromhex('000080d2'))
        at_site = lambda a: image.at(a, 4)
    else:
        # nop; nopl 0(%rax) and xorl %eax, %eax; nop; nop; nop, which
        # start a byte before the site, at the call's opcode
        sites = (bytes.fromhex('900f1f4000'), bytes.fromhex('33c0909090'))
        at_site = lambda a: image.at(a - 1, 5)

    for name, addr, size, foff, align, flags in image.sections:
        if flags & 0xFF != S_DTRACE_DOF:
            continue
        dof = image.data[foff:foff + size]
        nsecs = image.u32(foff + 28)
        headers = [struct.unpack_from('<IIIIQQ', dof, 64 + 32 * i) for i in range(nsecs)]
        by_type = {h[0]: h for h in headers}
        strtab = dof[by_type[DOF_SECT_STRTAB][4]:][:by_type[DOF_SECT_STRTAB][5]]

        def string(o):
            return strtab[o:strtab.index(b'\0', o)].decode()

        prov = by_type[DOF_SECT_PROVIDER][4]
        attrs = struct.unpack_from('<5I', dof, prov + 20)
        provider = string(struct.unpack_from('<I', dof, prov + 16)[0])
        print(f'dof {name} {provider} flags 0x{flags:x} align {align}')
        print('attrs ' + ' '.join(f'0x{a:08x}' for a in attrs))

        probes = by_type[DOF_SECT_PROBES]
        for k in range(probes[5] // 48):
            p = probes[4] + 48 * k
            func, pname, nargv = struct.unpack_from('<III', dof, p + 8)
            nargc, _, noffs = struct.unpack_from('<BBH', dof, p + 32)
            nenoffs = struct.unpack_from('<H', dof, p + 40)[0]
            args, o = [], nargv
            for _ in range(nargc):
                args.append(string(o))
                o += len(args[-1]) + 1
            print(f'probe {string(pname)}({", ".join(args)}) in {string(func)}: '
                  f'{noffs} sites, {nenoffs} tests')

        for ty, kind in ((DOF_SECT_PROFFS, 0), (DOF_SECT_PRENOFFS, 1)):
            if ty not in by_type:
                continue
            for k in range(by_type[ty][5] // 4):
                o = by_type[ty][4] + 4 * k
                site = addr + struct.unpack_from('<i', dof, o)[0]
                if at_site(site) != sites[kind]:
                    print(f'bad site 0x{site:x}: {at_site(site).hex()}')


main()
