#include "blake3.h"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

int main(int argc, char **argv) {
  // For each filepath argument, memory map it and hash it.
  for (int i = 1; i < argc; i++) {
    // Open and memory map the file.
    int fd = open(argv[i], O_RDONLY);
    if (fd == -1) {
      fprintf(stderr, "open failed: %s\n", strerror(errno));
      return 1;
    }
    struct stat statbuf;
    if (fstat(fd, &statbuf) == -1) {
      fprintf(stderr, "stat failed: %s\n", strerror(errno));
      return 1;
    }
    void *mapped = mmap(NULL, statbuf.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
    if (mapped == MAP_FAILED) {
      fprintf(stderr, "mmap failed: %s\n", strerror(errno));
      return 1;
    }

    // Initialize the hasher.
    blake3_hasher hasher;
    blake3_hasher_init(&hasher);

    // Hash the mapped file using multiple threads.
    blake3_hasher_update_tbb(&hasher, mapped, statbuf.st_size);

    // Unmap and close the file.
    if (munmap(mapped, statbuf.st_size) == -1) {
      fprintf(stderr, "munmap failed: %s\n", strerror(errno));
      return 1;
    }
    if (close(fd) == -1) {
      fprintf(stderr, "close failed: %s\n", strerror(errno));
      return 1;
    }

    // Finalize the hash. BLAKE3_OUT_LEN is the default output length, 32 bytes.
    uint8_t output[BLAKE3_OUT_LEN];
    blake3_hasher_finalize(&hasher, output, BLAKE3_OUT_LEN);

    // Print the hash as hexadecimal.
    for (size_t i = 0; i < BLAKE3_OUT_LEN; i++) {
      printf("%02x", output[i]);
    }
    printf("\n");
  }
}
