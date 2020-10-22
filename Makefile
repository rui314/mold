CC=clang
CXX=clang++
LLVM_CONFIG=llvm-project/build/bin/llvm-config
LLVM_TBLGEN=llvm-project/build/bin/llvm-tblgen
LLVM_LIBS=$(wildcard llvm-project/build/lib/libLLVM*.a)

CURRENT_DIR=$(shell pwd)
TBB_LIBDIR=$(wildcard $(CURRENT_DIR)/oneTBB/build/linux_intel64_*_release/)

CPPFLAGS=-g $(shell $(LLVM_CONFIG) --cxxflags) -IoneTBB/include -pthread -O2 -std=c++17
LDFLAGS=$(shell $(LLVM_CONFIG) --ldflags) -L$(TBB_LIBDIR) -Wl,-rpath=$(TBB_LIBDIR) -fuse-ld=lld
LIBS=-pthread -ltbb -lcurses -Wl,--start-group $(LLVM_LIBS) -Wl,--end-group
OBJS=main.o writer.o input_files.o input_chunks.o output_chunks.o output_file.o

mold: $(OBJS)
	$(CXX) $(CFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)

$(OBJS): mold.h Makefile

main.cc: options.inc

options.inc: options.td
	$(LLVM_TBLGEN) -I=llvm-project/llvm/include --gen-opt-parser-defs -o $@ $^

submodules: llvm intel_tbb

llvm:
	mkdir -p llvm-project/build
	(cd llvm-project/build; cmake -GNinja -DCMAKE_BUILD_TYPE=Release ../llvm)
	ninja -C llvm-project/build

intel_tbb:
	$(MAKE) -C oneTBB

test: mold
	./llvm-project/build/bin/llvm-lit test

clean:
	rm -f *.o *~ mold options.inc

.PHONY: llvm intel_tbb test clean
