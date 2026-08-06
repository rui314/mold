#!/usr/bin/env python3
# Work around a deadlock in portage's binary package extraction.
#
# tar_stream_reader.close() joins its stdin-feeder thread and then waits
# for the external decompressor to exit without draining the
# decompressor's remaining stdout. tarfile stops reading at the tar
# end-of-archive marker, so trailing archive padding is left in the
# pipe. If that residue exceeds the pipe buffer, the decompressor
# blocks writing forever and close() never returns, freezing the whole
# emerge. Small pipe buffers make this reachable in practice: the
# kernel shrinks new pipes to one page once a user exceeds
# fs.pipe-user-pages-soft, which our 80 parallel containers do.
#
# The fix is to drain the decompressor's stdout to EOF before joining
# and waiting. This script patches the installed gpkg.py at container
# image build time and fails loudly if the code no longer matches, so
# a portage upgrade that changes (or fixes) this code is noticed.

import sys
import portage.gpkg

path = portage.gpkg.__file__
src = open(path).read()

old = """        if self.proc is not None:
            self.thread.join()
"""

new = """        if self.proc is not None:
            # Drain remaining decompressor output before joining and
            # waiting; otherwise the decompressor can block forever on
            # a full stdout pipe. Injected by mold's lib/gentoo-test.sh.
            try:
                while self.read_io.read(65536):
                    pass
            except Exception:
                pass
            self.thread.join()
"""

if src.count(old) != 1:
    sys.exit(f"gpkg workaround: expected pattern not found in {path}; "
             "portage changed, update lib/gentoo-gpkg-workaround.py")

with open(path, "w") as f:
    f.write(src.replace(old, new))

import py_compile
py_compile.compile(path, doraise=True)
print(f"gpkg deadlock workaround applied to {path}")
