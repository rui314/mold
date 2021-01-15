CC=clang
CXX=clang++

CURRENT_DIR=$(shell pwd)
TBB_LIBDIR=$(wildcard $(CURRENT_DIR)/oneTBB/build/linux_intel64_*_release/)
JEMALLOC_LIBDIR=$(CURRENT_DIR)/jemalloc/lib

CPPFLAGS=-g -IoneTBB/include -pthread -std=c++20 -Wno-deprecated-volatile \
  -Wno-switch -fno-builtin-malloc -fno-builtin-calloc -fno-builtin-realloc \
  -fno-builtin-free -O2
LDFLAGS=-L$(TBB_LIBDIR) -Wl,-rpath=$(TBB_LIBDIR) \
  -L$(JEMALLOC_LIBDIR) -Wl,-rpath=$(JEMALLOC_LIBDIR) \
  -lcrypto -pthread
LIBS=-ltbb -ljemalloc
OBJS=main.o object_file.o input_sections.o output_chunks.o mapfile.o perf.o \
  linker_script.o archive_file.o sha1.o output_file.o subprocess.o

mold: $(OBJS)
	$(CXX) $(CFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)

$(OBJS): mold.h elf.h Makefile

submodules:
	$(MAKE) -C oneTBB
	(cd jemalloc; ./autogen.sh)
	$(MAKE) -C jemalloc

test: mold
	(cd test; for i in *.sh; do ./$$i || exit 1; done)

clean:
	rm -f *.o *~ mold

.PHONY: intel_tbb test clean
