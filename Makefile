CC=clang
CXX=clang++

CURRENT_DIR=$(shell pwd)
TBB_LIBDIR=$(wildcard $(CURRENT_DIR)/oneTBB/build/linux_intel64_*_release/)

CPPFLAGS=-g -IoneTBB/include -pthread -std=c++20 -Wno-deprecated-volatile -O2
LDFLAGS=-L$(TBB_LIBDIR) -Wl,-rpath=$(TBB_LIBDIR) -fuse-ld=lld
LIBS=-pthread -ltbb -lcurses
OBJS=main.o object_file.o input_sections.o output_chunks.o mapfile.o perf.o \
  linker_script.o

mold: $(OBJS)
	$(CXX) $(CFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)

$(OBJS): mold.h Makefile

submodules: intel_tbb

intel_tbb:
	$(MAKE) -C oneTBB

test: mold
	(cd test; for i in *.sh; do ./$$i || exit 1; done)

clean:
	rm -f *.o *~ mold

.PHONY: intel_tbb test clean
