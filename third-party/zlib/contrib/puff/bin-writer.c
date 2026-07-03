#include "stdio.h"
#include "stdlib.h"

/**
 * Reads hexadecimal values from stdin and writes binary bytes to stdout.
 * Accepts hex values separated by spaces, newlines, commas, etc.
 * Handles both uppercase and lowercase hex digits.
 */
int main(void) {
    char hexStr[3]; // Two hex digits + null terminator
    int ch;

    // Read characters until EOF
    while((ch = getchar()) != EOF) {
        hexStr[0] = (char)ch;
        hexStr[1] = (char)getchar();
        hexStr[2] = '\0'; // Null-terminate string
        char *endptr;
        unsigned char byte = (unsigned char)strtol(hexStr, &endptr, 16);
        fwrite(&byte, 1, 1, stdout);
        if((ch = getchar()) == EOF) // Read seaparating space
          break;
    }

    return 0;
}
