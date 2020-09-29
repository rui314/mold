CPPFLAGS=-g

catld: main.o
	$(CXX) $(CFLAGS) $^ -o $@ $(LDFLAGS)

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
	rm -f *.o *~ catld

.PHONY: llvm intel_tbb test clean
