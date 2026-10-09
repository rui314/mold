"""Reads and patches the 64-bit little-endian Mach-O files of the tests.

The tests' Python snippets `import macho` (common.inc puts this directory
on PYTHONPATH), and its command line makes the edits that several tests
make to their inputs:

  python3 -m macho set-section-flags FILE SEG SECT FLAGS [SEG SECT FLAGS...]
  python3 -m macho set-section-align FILE SEG SECT P2ALIGN
  python3 -m macho set-build-version FILE PLATFORM MINOS SDK

Each rewrites FILE in place and fails if FILE has no such section or
load command. Versions are given as X.Y[.Z].
"""

import struct
import sys

LC_SYMTAB = 0x2
LC_DYSYMTAB = 0xB
LC_SEGMENT_64 = 0x19
LC_CODE_SIGNATURE = 0x1D
LC_BUILD_VERSION = 0x32
LC_ATOM_INFO = 0x36
LC_LAZY_LOAD_DYLIB_INFO = 0x3A
LC_DYLD_EXPORTS_TRIE = 0x80000033

CPU_TYPE_ARM64 = 0x0100000C


class Segment:
    def __init__(self, data, hdr):
        self.hdr = hdr  # the file offset of the load command
        self.name = data[hdr + 8:hdr + 24].rstrip(b'\0').decode('latin-1')
        (self.vmaddr, self.vmsize, self.fileoff,
         self.filesize) = struct.unpack_from('<QQQQ', data, hdr + 24)


class Section:
    def __init__(self, data, hdr):
        self.hdr = hdr  # the file offset of the section header
        self.sectname = data[hdr:hdr + 16].rstrip(b'\0').decode('latin-1')
        self.segname = data[hdr + 16:hdr + 32].rstrip(b'\0').decode('latin-1')
        (self.addr, self.size, self.offset, self.align, self.reloff, self.nreloc,
         self.flags, self.reserved1, self.reserved2) = struct.unpack_from('<QQIIIIIII', data, hdr + 32)


class Symbol:
    def __init__(self, data, off, stroff):
        self.strx, self.type, self.sect, self.desc, self.value = struct.unpack_from('<IBBHQ', data, off)
        start = stroff + self.strx
        self.name = bytes(data[start:data.index(b'\0', start)])


class MachO:
    def __init__(self, path):
        self.data = bytearray(open(path, 'rb').read())
        self.cputype = self.u32(4)
        self.filetype = self.u32(12)
        self.commands = []  # (file offset, cmd, cmdsize)
        off = 32
        for _ in range(self.u32(16)):
            cmd, size = struct.unpack_from('<II', self.data, off)
            self.commands.append((off, cmd, size))
            off += size
        self.segments = [Segment(self.data, o) for o, cmd, _ in self.commands if cmd == LC_SEGMENT_64]
        self.sections = [Section(self.data, seg.hdr + 72 + 80 * i)
                         for seg in self.segments for i in range(self.u32(seg.hdr + 64))]

    def u32(self, off):
        return struct.unpack_from('<I', self.data, off)[0]

    def u64(self, off):
        return struct.unpack_from('<Q', self.data, off)[0]

    def set_u32(self, off, value):
        struct.pack_into('<I', self.data, off, value)

    def set_u64(self, off, value):
        struct.pack_into('<Q', self.data, off, value % 2**64)

    def save(self, path):
        open(path, 'wb').write(self.data)

    def command(self, cmd):
        """The file offset of the first load command of a kind, or None."""
        return next((o for o, c, _ in self.commands if c == cmd), None)

    def linkedit_data(self, cmd):
        """The data that a linkedit_data_command of a kind points at."""
        off = self.command(cmd)
        dataoff, datasize = struct.unpack_from('<II', self.data, off + 8)
        return bytes(self.data[dataoff:dataoff + datasize])

    def segment(self, name):
        return next(seg for seg in self.segments if seg.name == name)

    def section(self, sectname, segname=None):
        """The first section of a name (and segment, if given)."""
        return next(s for s in self.sections
                    if s.sectname == sectname and segname in (None, s.segname))

    def contents(self, sect):
        return bytes(self.data[sect.offset:sect.offset + sect.size])

    def at(self, addr, n):
        """The n bytes at a virtual address, or none if it is unmapped."""
        for seg in self.segments:
            if seg.vmaddr <= addr < seg.vmaddr + seg.vmsize:
                return bytes(self.data[seg.fileoff + addr - seg.vmaddr:][:n])
        return b''

    def relocs(self, sect):
        """A section's relocations as (file offset, r_address, r_info)."""
        return [(o, *struct.unpack_from('<II', self.data, o))
                for o in range(sect.reloff, sect.reloff + 8 * sect.nreloc, 8)]

    def symtab(self):
        """LC_SYMTAB's (symoff, nsyms, stroff, strsize)."""
        return struct.unpack_from('<IIII', self.data, self.command(LC_SYMTAB) + 8)

    def symbols(self):
        symoff, nsyms, stroff, _ = self.symtab()
        return [Symbol(self.data, symoff + 16 * i, stroff) for i in range(nsyms)]


def version(s):
    """Encodes X.Y[.Z] as Mach-O does, xxxx.yy.zz in nibbles."""
    parts = [int(x) for x in s.split('.')] + [0, 0]
    return parts[0] << 16 | parts[1] << 8 | parts[2]


def set_section_field(path, field, args):
    m = MachO(path)
    for i in range(0, len(args), 3):
        seg, sect, value = args[i], args[i + 1], int(args[i + 2], 0)
        matches = [s for s in m.sections if (s.segname, s.sectname) == (seg, sect)]
        if not matches:
            sys.exit(f'{path}: no section {seg},{sect}')
        for s in matches:
            m.set_u32(s.hdr + field, value)
    m.save(path)


def set_build_version(path, platform, minos, sdk):
    m = MachO(path)
    off = m.command(LC_BUILD_VERSION)
    if off is None:
        sys.exit(f'{path}: no LC_BUILD_VERSION')
    struct.pack_into('<III', m.data, off + 8, int(platform), version(minos), version(sdk))
    m.save(path)


def main(argv):
    op, path, args = argv[1], argv[2], argv[3:]
    if op == 'set-section-flags':
        set_section_field(path, 64, args)
    elif op == 'set-section-align':
        set_section_field(path, 52, args)
    elif op == 'set-build-version':
        set_build_version(path, *args)
    else:
        sys.exit(f'unknown operation: {op}')


if __name__ == '__main__':
    main(sys.argv)
