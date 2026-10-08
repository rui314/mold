#!/usr/bin/env python3
"""Prints the __unwind_info encoding the unwinder finds for addresses.

For each symbol or address (0x...) given after an image, it prints the
encoding as libunwind looks it up, the last entry at or below the
address, through the first-level index and a regular or compressed
second-level page, or "none" past the table's end.

Usage: unwind-lookup.py IMAGE SYMBOL|ADDRESS...
"""

import bisect
import struct
import sys

import macho

UNWIND_SECOND_LEVEL_COMPRESSED = 3


def main():
    m = macho.MachO(sys.argv[1])
    base = m.segment('__TEXT').vmaddr
    d = m.contents(m.section('__unwind_info'))
    # The defined symbols, as nm lists them.
    syms = {s.name.decode('latin-1'): s.value for s in m.symbols()
            if not s.type & 0xE0 and s.type & 0x0E}

    _, ceo, cec, _, _, iso, isc = struct.unpack_from('<7I', d, 0)
    common = struct.unpack_from(f'<{cec}I', d, ceo)
    index = [struct.unpack_from('<3I', d, iso + 12 * k) for k in range(isc)]

    for key in sys.argv[2:]:
        fo = (int(key, 16) if key.startswith('0x') else syms[key]) - base
        k = bisect.bisect_right([e[0] for e in index], fo) - 1
        if k < 0 or k >= isc - 1:
            print('none')
            continue
        first, page, _ = index[k]
        if struct.unpack_from('<I', d, page)[0] == UNWIND_SECOND_LEVEL_COMPRESSED:
            _, eo, ec, eco, ecc = struct.unpack_from('<IHHHH', d, page)
            local = struct.unpack_from(f'<{ecc}I', d, page + eco)
            ents = [(first + (e & 0xFFFFFF), e >> 24)
                    for e in struct.unpack_from(f'<{ec}I', d, page + eo)]
            ents = [(a, common[i] if i < cec else local[i - cec]) for a, i in ents]
        else:
            _, eo, ec = struct.unpack_from('<IHH', d, page)
            ents = [struct.unpack_from('<II', d, page + eo + 8 * j) for j in range(ec)]
        j = bisect.bisect_right([a for a, _ in ents], fo) - 1
        print(hex(ents[j][1]) if j >= 0 else 'none')


main()
