CC=clang
CXX=clang++
LLVM_CONFIG=llvm-project/build/bin/llvm-config
LLVM_TBLGEN=llvm-project/build/bin/llvm-tblgen

CPPFLAGS=-g $(shell $(LLVM_CONFIG) --cxxflags) -pthread
LDFLAGS=$(shell $(LLVM_CONFIG) --ldflags)
LIBS=-pthread -lLLVMSupport -lLLVMObject -lLLVMOption -lcurses
OBJS=main.o writer.o

catld: $(OBJS)
	$(CXX) $(CFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)

$(OBJS): catld.h Makefile

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

test: catld
	./llvm-project/build/bin/llvm-lit test

clean:
	rm -f *.o *~ catld options.inc

.PHONY: llvm intel_tbb test clean
