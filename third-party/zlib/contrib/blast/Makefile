all: test

libblast.so: blast.c blast.h
	cc -o libblast.so -shared blast.c

blast-test: libblast.so
	cc -o blast-test.o -c blast-test.c
	cc -o blast-test blast-test.o libblast.so

test: blast-test
	LD_LIBRARY_PATH=./ ./blast-test < test.pk | cmp - test.txt

clean:
	rm -f libblast.so blast-test blast-test.o
