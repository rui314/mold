#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A plugin that doesn't negotiate the API version makes mold restart
# itself. The command line is parsed again, so -C must be relative to the
# same directory as before.
test_cflags -flto || skip

cat <<EOF | cc -shared -fPIC -o $t/plugin.so -xc -
enum {
  LDPT_REGISTER_CLAIM_FILE_HOOK = 5,
  LDPT_REGISTER_ALL_SYMBOLS_READ_HOOK = 6,
};

struct tv {
  int tag;
  void *ptr;
};

static int claim_file(void *file, int *claimed) {
  *claimed = 1;
  return 0;
}

static int all_symbols_read(void) {
  return 0;
}

int onload(struct tv *tv) {
  for (; tv->tag; tv++) {
    if (tv->tag == LDPT_REGISTER_CLAIM_FILE_HOOK)
      ((int (*)(void *))tv->ptr)(claim_file);
    if (tv->tag == LDPT_REGISTER_ALL_SYMBOLS_READ_HOOK)
      ((int (*)(void *))tv->ptr)(all_symbols_read);
  }
  return 0;
}
EOF

cat <<EOF | $CC -c -o $t/a.o -xc -
void _start() {}
EOF

mkdir -p $t/dir
echo -e 'BC\xc0\xde' > $t/dir/b.bc
rm -f $t/dir/exe

./mold -C $t/dir -plugin $PWD/$t/plugin.so $PWD/$t/a.o b.bc -o exe
readelf -Ws $t/dir/exe | grep -E ' _start( |$)'
