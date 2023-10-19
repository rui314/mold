// FIXME(eddyb) should this use `<rust-demangle.h>`?
#include "rust-demangle.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct rust_demangler {
    const char *sym;
    size_t sym_len;

    void *callback_opaque;
    void (*callback)(const char *data, size_t len, void *opaque);

    // Position of the next character to read from the symbol.
    size_t next;

    // `true` if any error occurred.
    bool errored;

    // `true` if nothing should be printed.
    bool skipping_printing;

    // `true` if printing should be verbose (e.g. include hashes).
    bool verbose;

    // Rust mangling version, with legacy mangling being -1.
    int version;

    uint64_t bound_lifetime_depth;
};

#define ERROR_AND(x)                                                           \
    do {                                                                       \
        rdm->errored = true;                                                   \
        x;                                                                     \
    } while (0)
#define CHECK_OR(cond, x)                                                      \
    do {                                                                       \
        if (!(cond))                                                           \
            ERROR_AND(x);                                                      \
    } while (0)

// FIXME(eddyb) consider renaming these to not start with `IS` (UB?).
#define IS_DIGIT(c) ((c) >= '0' && (c) <= '9')
#define IS_UPPER(c) ((c) >= 'A' && (c) <= 'Z')
#define IS_LOWER(c) ((c) >= 'a' && (c) <= 'z')

// Parsing functions.

static char peek(const struct rust_demangler *rdm) {
    if (rdm->next < rdm->sym_len)
        return rdm->sym[rdm->next];
    return 0;
}

static bool eat(struct rust_demangler *rdm, char c) {
    if (peek(rdm) == c) {
        rdm->next++;
        return true;
    } else
        return false;
}

static char next(struct rust_demangler *rdm) {
    char c = peek(rdm);
    CHECK_OR(c, return 0);
    rdm->next++;
    return c;
}

struct hex_nibbles {
    const char *nibbles;
    size_t nibbles_len;
};

static struct hex_nibbles parse_hex_nibbles(struct rust_demangler *rdm) {
    struct hex_nibbles hex;

    hex.nibbles = NULL;
    hex.nibbles_len = 0;

    size_t start = rdm->next, hex_len = 0;
    while (!eat(rdm, '_')) {
        char c = next(rdm);
        CHECK_OR(IS_DIGIT(c) || (c >= 'a' && c <= 'f'), return hex);
        hex_len++;
    }

    hex.nibbles = rdm->sym + start;
    hex.nibbles_len = hex_len;
    return hex;
}

static struct hex_nibbles
parse_hex_nibbles_for_const_uint(struct rust_demangler *rdm) {
    struct hex_nibbles hex = parse_hex_nibbles(rdm);
    CHECK_OR(!rdm->errored, return hex);

    // Trim leading `0`s.
    while (hex.nibbles_len > 0 && *hex.nibbles == '0') {
        hex.nibbles++;
        hex.nibbles_len--;
    }

    return hex;
}

static struct hex_nibbles
parse_hex_nibbles_for_const_bytes(struct rust_demangler *rdm) {
    struct hex_nibbles hex = parse_hex_nibbles(rdm);
    CHECK_OR(!rdm->errored && (hex.nibbles_len % 2 == 0), return hex);
    return hex;
}

static uint8_t decode_hex_nibble(char nibble) {
    return nibble >= 'a' ? 10 + (nibble - 'a') : nibble - '0';
}

static uint64_t parse_integer_62(struct rust_demangler *rdm) {
    if (eat(rdm, '_'))
        return 0;

    uint64_t x = 0;
    while (!eat(rdm, '_')) {
        char c = next(rdm);
        x *= 62;
        if (IS_DIGIT(c))
            x += c - '0';
        else if (IS_LOWER(c))
            x += 10 + (c - 'a');
        else if (IS_UPPER(c))
            x += 10 + 26 + (c - 'A');
        else
            ERROR_AND(return 0);
    }
    return x + 1;
}

static uint64_t parse_opt_integer_62(struct rust_demangler *rdm, char tag) {
    if (!eat(rdm, tag))
        return 0;
    return 1 + parse_integer_62(rdm);
}

static uint64_t parse_disambiguator(struct rust_demangler *rdm) {
    return parse_opt_integer_62(rdm, 's');
}

struct rust_mangled_ident {
    // ASCII part of the identifier.
    const char *ascii;
    size_t ascii_len;

    // Punycode insertion codes for Unicode codepoints, if any.
    const char *punycode;
    size_t punycode_len;
};

static struct rust_mangled_ident parse_ident(struct rust_demangler *rdm) {
    struct rust_mangled_ident ident;

    ident.ascii = NULL;
    ident.ascii_len = 0;
    ident.punycode = NULL;
    ident.punycode_len = 0;

    bool is_punycode = false;
    if (rdm->version != -1) {
        is_punycode = eat(rdm, 'u');
    }

    char c = next(rdm);
    CHECK_OR(IS_DIGIT(c), return ident);
    size_t len = c - '0';

    if (c != '0')
        while (IS_DIGIT(peek(rdm)))
            len = len * 10 + (next(rdm) - '0');

    if (rdm->version != -1) {
        // Skip past the optional `_` separator.
        eat(rdm, '_');
    }

    size_t start = rdm->next;
    rdm->next += len;
    // Check for overflows.
    CHECK_OR((start <= rdm->next) && (rdm->next <= rdm->sym_len), return ident);

    ident.ascii = rdm->sym + start;
    ident.ascii_len = len;

    if (is_punycode) {
        ident.punycode_len = 0;
        while (ident.ascii_len > 0) {
            ident.ascii_len--;

            // The last '_' is a separator between ascii & punycode.
            if (ident.ascii[ident.ascii_len] == '_')
                break;

            ident.punycode_len++;
        }
        CHECK_OR(ident.punycode_len > 0, return ident);
        ident.punycode = ident.ascii + (len - ident.punycode_len);
    }

    if (ident.ascii_len == 0)
        ident.ascii = NULL;

    return ident;
}

// Printing functions.

static void
print_str(struct rust_demangler *rdm, const char *data, size_t len) {
    if (!rdm->errored && !rdm->skipping_printing)
        rdm->callback(data, len, rdm->callback_opaque);
}

#define PRINT(s) print_str(rdm, s, strlen(s))

static void print_uint64(struct rust_demangler *rdm, uint64_t x) {
    char s[21];
    sprintf(s, "%" PRIu64, x);
    PRINT(s);
}

static void print_uint64_hex(struct rust_demangler *rdm, uint64_t x) {
    char s[17];
    sprintf(s, "%" PRIx64, x);
    PRINT(s);
}

static void
print_quoted_escaped_char(struct rust_demangler *rdm, char quote, uint32_t c) {
    CHECK_OR(c < 0xd800 || (c > 0xdfff && c < 0x10ffff), return);

    switch (c) {
    case '\0':
        PRINT("\\0");
        break;

    case '\t':
        PRINT("\\t");
        break;

    case '\r':
        PRINT("\\r");
        break;

    case '\n':
        PRINT("\\n");
        break;

    case '\\':
        PRINT("\\\\");
        break;

    case '"':
        if (quote == '"') {
            PRINT("\\\"");
        } else {
            PRINT("\"");
        }
        break;

    case '\'':
        if (quote == '\'') {
            PRINT("\\'");
        } else {
            PRINT("'");
        }
        break;

    default:
        if (c >= 0x20 && c <= 0x7e) {
            // Printable ASCII
            char v = (char)c;
            print_str(rdm, &v, 1);
        } else {
            // FIXME show printable unicode characters without hex encoding
            PRINT("\\u{");
            char s[9] = {0};
            sprintf(s, "%" PRIx32, c);
            PRINT(s);
            PRINT("}");
        }
    }
}

static void
print_ident(struct rust_demangler *rdm, struct rust_mangled_ident ident) {
    if (rdm->errored || rdm->skipping_printing)
        return;

    if (!ident.punycode) {
        print_str(rdm, ident.ascii, ident.ascii_len);
        return;
    }

    size_t len = 0;
    size_t cap = 4;
    while (cap < ident.ascii_len) {
        cap *= 2;
        // Check for overflows.
        CHECK_OR((cap * 4) / 4 == cap, return);
    }

    // Store the output codepoints as groups of 4 UTF-8 bytes.
    uint8_t *out = (uint8_t *)malloc(cap * 4);
    CHECK_OR(out, return);

    // Populate initial output from ASCII fragment.
    for (len = 0; len < ident.ascii_len; len++) {
        uint8_t *p = out + 4 * len;
        p[0] = 0;
        p[1] = 0;
        p[2] = 0;
        p[3] = ident.ascii[len];
    }

    // Punycode parameters and initial state.
    size_t base = 36;
    size_t t_min = 1;
    size_t t_max = 26;
    size_t skew = 38;
    size_t damp = 700;
    size_t bias = 72;
    size_t i = 0;
    uint32_t c = 0x80;

    size_t punycode_pos = 0;
    while (punycode_pos < ident.punycode_len) {
        // Read one delta value.
        size_t delta = 0;
        size_t w = 1;
        size_t k = 0;
        size_t t;
        uint8_t d;
        do {
            k += base;
            t = k < bias ? 0 : (k - bias);
            if (t < t_min)
                t = t_min;
            if (t > t_max)
                t = t_max;

            CHECK_OR(punycode_pos < ident.punycode_len, goto cleanup);
            d = ident.punycode[punycode_pos++];

            if (IS_LOWER(d))
                d = d - 'a';
            else if (IS_DIGIT(d))
                d = 26 + (d - '0');
            else
                ERROR_AND(goto cleanup);

            delta += d * w;
            w *= base - t;
        } while (d >= t);

        // Compute the new insert position and character.
        len++;
        i += delta;
        c += i / len;
        i %= len;

        // Ensure enough space is available.
        if (cap < len) {
            cap *= 2;
            // Check for overflows.
            CHECK_OR((cap * 4) / 4 == cap, goto cleanup);
            CHECK_OR(cap >= len, goto cleanup);
        }
        uint8_t *p = (uint8_t *)realloc(out, cap * 4);
        CHECK_OR(p, goto cleanup);
        out = p;

        // Move the characters after the insert position.
        p = out + i * 4;
        memmove(p + 4, p, (len - i - 1) * 4);

        // Insert the new character, as UTF-8 bytes.
        p[0] = c >= 0x10000 ? 0xf0 | (c >> 18) : 0;
        p[1] =
            c >= 0x800 ? (c < 0x10000 ? 0xe0 : 0x80) | ((c >> 12) & 0x3f) : 0;
        p[2] = (c < 0x800 ? 0xc0 : 0x80) | ((c >> 6) & 0x3f);
        p[3] = 0x80 | (c & 0x3f);

        // If there are no more deltas, decoding is complete.
        if (punycode_pos == ident.punycode_len)
            break;

        i++;

        // Perform bias adaptation.
        delta /= damp;
        damp = 2;

        delta += delta / len;
        k = 0;
        while (delta > ((base - t_min) * t_max) / 2) {
            delta /= base - t_min;
            k += base;
        }
        bias = k + ((base - t_min + 1) * delta) / (delta + skew);
    }

    // Remove all the 0 bytes to leave behind an UTF-8 string.
    size_t j;
    for (i = 0, j = 0; i < len * 4; i++)
        if (out[i] != 0)
            out[j++] = out[i];

    print_str(rdm, (const char *)out, j);

cleanup:
    free(out);
}

/// Print the lifetime according to the previously decoded index.
/// An index of `0` always refers to `'_`, but starting with `1`,
/// indices refer to late-bound lifetimes introduced by a binder.
static void print_lifetime_from_index(struct rust_demangler *rdm, uint64_t lt) {
    PRINT("'");
    if (lt == 0) {
        PRINT("_");
        return;
    }

    uint64_t depth = rdm->bound_lifetime_depth - lt;
    // Try to print lifetimes alphabetically first.
    if (depth < 26) {
        char c = 'a' + depth;
        print_str(rdm, &c, 1);
    } else {
        // Use `'_123` after running out of letters.
        PRINT("_");
        print_uint64(rdm, depth);
    }
}

// Demangling functions.

static void demangle_binder(struct rust_demangler *rdm);
static void demangle_path(struct rust_demangler *rdm, bool in_value);
static void demangle_generic_arg(struct rust_demangler *rdm);
static void demangle_type(struct rust_demangler *rdm);
static bool demangle_path_maybe_open_generics(struct rust_demangler *rdm);
static void demangle_dyn_trait(struct rust_demangler *rdm);
static void demangle_const(struct rust_demangler *rdm, bool in_value);
static void demangle_const_uint(struct rust_demangler *rdm, char ty_tag);
static void demangle_const_str_literal(struct rust_demangler *rdm);

/// Optionally enter a binder ('G') for late-bound lifetimes,
/// printing e.g. `for<'a, 'b> `, and make those lifetimes visible
/// to the caller (via depth level, which the caller should reset).
static void demangle_binder(struct rust_demangler *rdm) {
    CHECK_OR(!rdm->errored, return);

    uint64_t bound_lifetimes = parse_opt_integer_62(rdm, 'G');
    if (bound_lifetimes > 0) {
        PRINT("for<");
        for (uint64_t i = 0; i < bound_lifetimes; i++) {
            if (i > 0)
                PRINT(", ");
            rdm->bound_lifetime_depth++;
            print_lifetime_from_index(rdm, 1);
        }
        PRINT("> ");
    }
}

static void demangle_path(struct rust_demangler *rdm, bool in_value) {
    CHECK_OR(!rdm->errored, return);

    char tag = next(rdm);
    switch (tag) {
    case 'C': {
        uint64_t dis = parse_disambiguator(rdm);
        struct rust_mangled_ident name = parse_ident(rdm);

        print_ident(rdm, name);
        if (rdm->verbose) {
            PRINT("[");
            print_uint64_hex(rdm, dis);
            PRINT("]");
        }
        break;
    }
    case 'N': {
        char ns = next(rdm);
        CHECK_OR(IS_LOWER(ns) || IS_UPPER(ns), return);

        demangle_path(rdm, in_value);

        uint64_t dis = parse_disambiguator(rdm);
        struct rust_mangled_ident name = parse_ident(rdm);

        if (IS_UPPER(ns)) {
            // Special namespaces, like closures and shims.
            PRINT("::{");
            switch (ns) {
            case 'C':
                PRINT("closure");
                break;
            case 'S':
                PRINT("shim");
                break;
            default:
                print_str(rdm, &ns, 1);
            }
            if (name.ascii || name.punycode) {
                PRINT(":");
                print_ident(rdm, name);
            }
            PRINT("#");
            print_uint64(rdm, dis);
            PRINT("}");
        } else {
            // Implementation-specific/unspecified namespaces.

            if (name.ascii || name.punycode) {
                PRINT("::");
                print_ident(rdm, name);
            }
        }
        break;
    }
    case 'M':
    case 'X':
        // Ignore the `impl`'s own path.
        parse_disambiguator(rdm);
        bool was_skipping_printing = rdm->skipping_printing;
        rdm->skipping_printing = true;
        demangle_path(rdm, in_value);
        rdm->skipping_printing = was_skipping_printing;
        __attribute__((fallthrough));
    case 'Y':
        PRINT("<");
        demangle_type(rdm);
        if (tag != 'M') {
            PRINT(" as ");
            demangle_path(rdm, false);
        }
        PRINT(">");
        break;
    case 'I':
        demangle_path(rdm, in_value);
        if (in_value)
            PRINT("::");
        PRINT("<");
        for (size_t i = 0; !rdm->errored && !eat(rdm, 'E'); i++) {
            if (i > 0)
                PRINT(", ");
            demangle_generic_arg(rdm);
        }
        PRINT(">");
        break;
    case 'B': {
        size_t backref = parse_integer_62(rdm);
        if (!rdm->skipping_printing) {
            size_t old_next = rdm->next;
            rdm->next = backref;
            demangle_path(rdm, in_value);
            rdm->next = old_next;
        }
        break;
    }
    default:
        ERROR_AND(return);
    }
}

static void demangle_generic_arg(struct rust_demangler *rdm) {
    if (eat(rdm, 'L')) {
        uint64_t lt = parse_integer_62(rdm);
        print_lifetime_from_index(rdm, lt);
    } else if (eat(rdm, 'K'))
        demangle_const(rdm, false);
    else
        demangle_type(rdm);
}

static const char *basic_type(char tag) {
    switch (tag) {
    case 'b':
        return "bool";
    case 'c':
        return "char";
    case 'e':
        return "str";
    case 'u':
        return "()";
    case 'a':
        return "i8";
    case 's':
        return "i16";
    case 'l':
        return "i32";
    case 'x':
        return "i64";
    case 'n':
        return "i128";
    case 'i':
        return "isize";
    case 'h':
        return "u8";
    case 't':
        return "u16";
    case 'm':
        return "u32";
    case 'y':
        return "u64";
    case 'o':
        return "u128";
    case 'j':
        return "usize";
    case 'f':
        return "f32";
    case 'd':
        return "f64";
    case 'z':
        return "!";
    case 'p':
        return "_";
    case 'v':
        return "...";

    default:
        return NULL;
    }
}

static void demangle_type(struct rust_demangler *rdm) {
    CHECK_OR(!rdm->errored, return);

    char tag = next(rdm);

    const char *basic = basic_type(tag);
    if (basic) {
        PRINT(basic);
        return;
    }

    switch (tag) {
    case 'R':
    case 'Q':
        PRINT("&");
        if (eat(rdm, 'L')) {
            uint64_t lt = parse_integer_62(rdm);
            if (lt) {
                print_lifetime_from_index(rdm, lt);
                PRINT(" ");
            }
        }
        if (tag != 'R')
            PRINT("mut ");
        demangle_type(rdm);
        break;
    case 'P':
    case 'O':
        PRINT("*");
        if (tag != 'P')
            PRINT("mut ");
        else
            PRINT("const ");
        demangle_type(rdm);
        break;
    case 'A':
    case 'S':
        PRINT("[");
        demangle_type(rdm);
        if (tag == 'A') {
            PRINT("; ");
            demangle_const(rdm, true);
        }
        PRINT("]");
        break;
    case 'T': {
        PRINT("(");
        size_t i;
        for (i = 0; !rdm->errored && !eat(rdm, 'E'); i++) {
            if (i > 0)
                PRINT(", ");
            demangle_type(rdm);
        }
        if (i == 1)
            PRINT(",");
        PRINT(")");
        break;
    }
    case 'F': {
        uint64_t old_bound_lifetime_depth = rdm->bound_lifetime_depth;
        demangle_binder(rdm);

        if (eat(rdm, 'U'))
            PRINT("unsafe ");

        if (eat(rdm, 'K')) {
            struct rust_mangled_ident abi;

            if (eat(rdm, 'C')) {
                abi.ascii = "C";
                abi.ascii_len = 1;
            } else {
                abi = parse_ident(rdm);
                CHECK_OR(abi.ascii && !abi.punycode, goto restore);
            }

            PRINT("extern \"");

            // If the ABI had any `-`, they were replaced with `_`,
            // so the parts between `_` have to be re-joined with `-`.
            for (size_t i = 0; i < abi.ascii_len; i++) {
                if (abi.ascii[i] == '_') {
                    print_str(rdm, abi.ascii, i);
                    PRINT("-");
                    abi.ascii += i + 1;
                    abi.ascii_len -= i + 1;
                    i = 0;
                }
            }
            print_str(rdm, abi.ascii, abi.ascii_len);

            PRINT("\" ");
        }

        PRINT("fn(");
        for (size_t i = 0; !rdm->errored && !eat(rdm, 'E'); i++) {
            if (i > 0)
                PRINT(", ");
            demangle_type(rdm);
        }
        PRINT(")");

        if (eat(rdm, 'u')) {
            // Skip printing the return type if it's 'u', i.e. `()`.
        } else {
            PRINT(" -> ");
            demangle_type(rdm);
        }

    // Restore `bound_lifetime_depth` to outside the binder.
    restore:
        rdm->bound_lifetime_depth = old_bound_lifetime_depth;
        break;
    }
    case 'D':
        PRINT("dyn ");

        uint64_t old_bound_lifetime_depth = rdm->bound_lifetime_depth;
        demangle_binder(rdm);

        for (size_t i = 0; !rdm->errored && !eat(rdm, 'E'); i++) {
            if (i > 0)
                PRINT(" + ");
            demangle_dyn_trait(rdm);
        }

        // Restore `bound_lifetime_depth` to outside the binder.
        rdm->bound_lifetime_depth = old_bound_lifetime_depth;

        CHECK_OR(eat(rdm, 'L'), return);
        uint64_t lt = parse_integer_62(rdm);
        if (lt) {
            PRINT(" + ");
            print_lifetime_from_index(rdm, lt);
        }
        break;
    case 'B': {
        size_t backref = parse_integer_62(rdm);
        if (!rdm->skipping_printing) {
            size_t old_next = rdm->next;
            rdm->next = backref;
            demangle_type(rdm);
            rdm->next = old_next;
        }
        break;
    }
    default:
        // Go back to the tag, so `demangle_path` also sees it.
        rdm->next--;
        demangle_path(rdm, false);
    }
}

/// A trait in a trait object may have some "existential projections"
/// (i.e. associated type bindings) after it, which should be printed
/// in the `<...>` of the trait, e.g. `dyn Trait<T, U, Assoc=X>`.
/// To this end, this method will keep the `<...>` of an 'I' path
/// open, by omitting the `>`, and return `Ok(true)` in that case.
static bool demangle_path_maybe_open_generics(struct rust_demangler *rdm) {
    bool open = false;

    CHECK_OR(!rdm->errored, return open);

    if (eat(rdm, 'B')) {
        size_t backref = parse_integer_62(rdm);
        if (!rdm->skipping_printing) {
            size_t old_next = rdm->next;
            rdm->next = backref;
            open = demangle_path_maybe_open_generics(rdm);
            rdm->next = old_next;
        }
    } else if (eat(rdm, 'I')) {
        demangle_path(rdm, false);
        PRINT("<");
        open = true;
        for (size_t i = 0; !rdm->errored && !eat(rdm, 'E'); i++) {
            if (i > 0)
                PRINT(", ");
            demangle_generic_arg(rdm);
        }
    } else
        demangle_path(rdm, false);
    return open;
}

static void demangle_dyn_trait(struct rust_demangler *rdm) {
    CHECK_OR(!rdm->errored, return);

    bool open = demangle_path_maybe_open_generics(rdm);

    while (eat(rdm, 'p')) {
        if (!open)
            PRINT("<");
        else
            PRINT(", ");
        open = true;

        struct rust_mangled_ident name = parse_ident(rdm);
        print_ident(rdm, name);
        PRINT(" = ");
        demangle_type(rdm);
    }

    if (open)
        PRINT(">");
}

static void demangle_const(struct rust_demangler *rdm, bool in_value) {
    CHECK_OR(!rdm->errored, return);

    bool opened_brace = false;

    char ty_tag = next(rdm);
    switch (ty_tag) {
    case 'p':
        PRINT("_");
        break;

    // Unsigned integer types.
    case 'h':
    case 't':
    case 'm':
    case 'y':
    case 'o':
    case 'j':
        demangle_const_uint(rdm, ty_tag);
        break;

    case 'a':
    case 's':
    case 'l':
    case 'x':
    case 'n':
    case 'i':
        if (eat(rdm, 'n')) {
            PRINT("-");
        }
        demangle_const_uint(rdm, ty_tag);
        break;

    case 'b': {
        struct hex_nibbles hex = parse_hex_nibbles_for_const_uint(rdm);
        CHECK_OR(!rdm->errored && hex.nibbles_len <= 1, return);
        uint8_t v = hex.nibbles_len > 0 ? decode_hex_nibble(hex.nibbles[0]) : 0;
        CHECK_OR(v <= 1, return);
        PRINT(v == 1 ? "true" : "false");
        break;
    }

    case 'c': {
        struct hex_nibbles hex = parse_hex_nibbles_for_const_uint(rdm);
        CHECK_OR(!rdm->errored && hex.nibbles_len <= 6, return);

        uint32_t c = 0;
        for (size_t i = 0; i < hex.nibbles_len; i++)
            c = (c << 4) | decode_hex_nibble(hex.nibbles[i]);

        PRINT("'");
        print_quoted_escaped_char(rdm, '\'', c);
        PRINT("'");

        break;
    }

    case 'e':
        // NOTE(eddyb) a string literal `"..."` has type `&str`, so
        // to get back the type `str`, `*"..."` syntax is needed
        // (even if that may not be valid in Rust itself).
        if (!in_value) {
            opened_brace = true;
            PRINT("{");
        }
        PRINT("*");

        demangle_const_str_literal(rdm);
        break;

    case 'R':
    case 'Q':
        if (ty_tag == 'R' && eat(rdm, 'e')) {
            // NOTE(eddyb) this prints `"..."` instead of `&*"..."`, which
            // is what `Re..._` would imply (see comment for `str` above).
            demangle_const_str_literal(rdm);
            break;
        }

        if (!in_value) {
            opened_brace = true;
            PRINT("{");
        }

        PRINT("&");
        if (ty_tag != 'R') {
            PRINT("mut ");
        }

        demangle_const(rdm, true);
        break;

    case 'A': {
        if (!in_value) {
            opened_brace = true;
            PRINT("{");
        }

        PRINT("[");

        size_t i = 0;
        while (!eat(rdm, 'E')) {
            CHECK_OR(!rdm->errored, return);

            if (i > 0)
                PRINT(", ");

            demangle_const(rdm, true);

            i += 1;
        }

        PRINT("]");
        break;
    }

    case 'T': {
        if (!in_value) {
            opened_brace = true;
            PRINT("{");
        }

        PRINT("(");

        size_t i = 0;
        while (!eat(rdm, 'E')) {
            CHECK_OR(!rdm->errored, return);

            if (i > 0)
                PRINT(", ");

            demangle_const(rdm, true);

            i += 1;
        }

        if (i == 1)
            PRINT(",");

        PRINT(")");
        break;
    }

    case 'V':
        if (!in_value) {
            opened_brace = true;
            PRINT("{");
        }

        demangle_path(rdm, true);

        switch (next(rdm)) {
        case 'U':
            break;

        case 'T': {
            PRINT("(");

            size_t i = 0;
            while (!eat(rdm, 'E')) {
                CHECK_OR(!rdm->errored, return);

                if (i > 0)
                    PRINT(", ");

                demangle_const(rdm, true);

                i += 1;
            }

            PRINT(")");
            break;
        }

        case 'S': {
            PRINT(" { ");

            size_t i = 0;
            while (!eat(rdm, 'E')) {
                CHECK_OR(!rdm->errored, return);

                if (i > 0)
                    PRINT(", ");

                parse_disambiguator(rdm);

                struct rust_mangled_ident name = parse_ident(rdm);
                print_ident(rdm, name);

                PRINT(": ");

                demangle_const(rdm, true);

                i += 1;
            }

            PRINT(" }");
            break;
        }

        default:
            ERROR_AND(return);
        }

        break;

    case 'B': {
        size_t backref = parse_integer_62(rdm);
        if (!rdm->skipping_printing) {
            size_t old_next = rdm->next;
            rdm->next = backref;
            demangle_const(rdm, in_value);
            rdm->next = old_next;
        }
        break;
    }

    default:
        ERROR_AND(return);
    }

    if (opened_brace) {
        PRINT("}");
    }
}

static void demangle_const_uint(struct rust_demangler *rdm, char ty_tag) {
    CHECK_OR(!rdm->errored, return);

    struct hex_nibbles hex = parse_hex_nibbles_for_const_uint(rdm);
    CHECK_OR(!rdm->errored, return);

    // Print anything that doesn't fit in `uint64_t` verbatim.
    if (hex.nibbles_len > 16) {
        PRINT("0x");
        print_str(rdm, hex.nibbles, hex.nibbles_len);
    } else {
        uint64_t v = 0;
        for (size_t i = 0; i < hex.nibbles_len; i++)
            v = (v << 4) | decode_hex_nibble(hex.nibbles[i]);
        print_uint64(rdm, v);
    }

    if (rdm->verbose)
        PRINT(basic_type(ty_tag));
}

// UTF-8 uses an unary encoding for its "length" field (`1`s followed by a `0`).
struct utf8_byte {
    // Decoded "length" field of an UTF-8 byte, including the special cases:
    // - `0` indicates this is a lone ASCII byte
    // - `1` indicates a continuation byte (cannot start an UTF-8 sequence)
    size_t seq_len;

    // Remaining (`payload_width`) bits in the UTF-8 byte, contributing to
    // the Unicode scalar value being encoded in the UTF-8 sequence.
    uint8_t payload;
    size_t payload_width;
};
static struct utf8_byte utf8_decode(uint8_t byte) {
    struct utf8_byte utf8;

    utf8.seq_len = 0;
    utf8.payload = byte;
    utf8.payload_width = 8;

    // FIXME(eddyb) figure out if using "count leading ones/zeros" is an option.
    while (utf8.seq_len <= 6) {
        uint8_t msb = 0x80 >> utf8.seq_len;
        utf8.payload &= ~msb;
        utf8.payload_width--;
        if ((byte & msb) == 0)
            break;
        utf8.seq_len++;
    }

    return utf8;
}

static void demangle_const_str_literal(struct rust_demangler *rdm) {
    CHECK_OR(!rdm->errored, return);

    struct hex_nibbles hex = parse_hex_nibbles_for_const_bytes(rdm);
    CHECK_OR(!rdm->errored, return);

    PRINT("\"");
    for (size_t i = 0; i < hex.nibbles_len; i += 2) {
        struct utf8_byte utf8 = utf8_decode(
            (decode_hex_nibble(hex.nibbles[i]) << 4) |
            decode_hex_nibble(hex.nibbles[i + 1])
        );
        uint32_t c = utf8.payload;
        if (utf8.seq_len > 0) {
            CHECK_OR(utf8.seq_len >= 2 && utf8.seq_len <= 4, return);
            for (size_t extra = utf8.seq_len - 1; extra > 0; extra--) {
                i += 2;
                utf8 = utf8_decode(
                    (decode_hex_nibble(hex.nibbles[i]) << 4) |
                    decode_hex_nibble(hex.nibbles[i + 1])
                );
                CHECK_OR(utf8.seq_len == 1, return);
                c = (c << utf8.payload_width) | utf8.payload;
            }
        }
        print_quoted_escaped_char(rdm, '"', c);
    }
    PRINT("\"");
}

static bool is_rust_hash(struct rust_mangled_ident name) {
    if (name.ascii[0] != 'h') {
        return false;
    }
    for (size_t i = 1; i < name.ascii_len; i++) {
        if (!IS_DIGIT(name.ascii[i]) &&
            !(name.ascii[i] >= 'a' && name.ascii[i] <= 'f')) {
            return false;
        }
    }
    return true;
}

static void print_legacy_ident(
    struct rust_demangler *rdm, struct rust_mangled_ident ident
) {
    if (rdm->errored || rdm->skipping_printing)
        return;

    CHECK_OR(!ident.punycode, return);

    if (ident.ascii[0] == '_' && ident.ascii[1] == '$') {
        ident.ascii += 1;
        ident.ascii_len -= 1;
    }

    while (1) {
        if (ident.ascii_len == 0) {
            break;
        } else if (ident.ascii[0] == '.') {
            if (ident.ascii_len >= 2 && ident.ascii[1] == '.') {
                PRINT("::");
                ident.ascii += 2;
                ident.ascii_len -= 2;
            } else {
                PRINT(".");
                ident.ascii += 1;
                ident.ascii_len -= 1;
            }
        } else if (ident.ascii[0] == '$') {
            const char *end_ptr =
                (const char *)memchr(&ident.ascii[1], '$', ident.ascii_len - 1);
            if (!end_ptr)
                break;
            const char *escape = &ident.ascii[1];
            size_t escape_len = end_ptr - escape;

            if (strncmp(escape, "SP", 2) == 0) {
                PRINT("@");
            } else if (strncmp(escape, "BP", 2) == 0) {
                PRINT("*");
            } else if (strncmp(escape, "RF", 2) == 0) {
                PRINT("&");
            } else if (strncmp(escape, "LT", 2) == 0) {
                PRINT("<");
            } else if (strncmp(escape, "GT", 2) == 0) {
                PRINT(">");
            } else if (strncmp(escape, "LP", 2) == 0) {
                PRINT("(");
            } else if (strncmp(escape, "RP", 2) == 0) {
                PRINT(")");
            } else if (strncmp(escape, "C", 1) == 0) {
                PRINT(",");
            } else {
                if (escape[0] != 'u') {
                    break;
                }

                const char *digits = &escape[1];
                size_t digits_len = escape_len - 1;

                bool invalid = false;
                for (size_t i = 1; i < digits_len; i++) {
                    if (!IS_DIGIT(digits[i]) &&
                        !(digits[i] >= 'a' && digits[i] <= 'f')) {
                        invalid = true;
                        break;
                    }
                }
                if (invalid)
                    break;

                struct hex_nibbles hex;

                hex.nibbles = digits;
                hex.nibbles_len = digits_len;

                uint32_t c = 0;
                for (size_t i = 0; i < hex.nibbles_len; i++)
                    c = (c << 4) | decode_hex_nibble(hex.nibbles[i]);

                if (!(c < 0xd800 || (c > 0xdfff && c < 0x10ffff))) {
                    break; // Not a valid unicode scalar
                }

                if (c >= 0x20 && c <= 0x7e) {
                    // Printable ASCII
                    char v = (char)c;
                    print_str(rdm, &v, 1);
                } else {
                    // FIXME show printable unicode characters without hex
                    // encoding
                    PRINT("\\u{");
                    char s[9] = {0};
                    sprintf(s, "%" PRIx32, c);
                    PRINT(s);
                    PRINT("}");
                }
            }

            ident.ascii += escape_len + 2;
            ident.ascii_len -= escape_len + 2;
        } else {
            bool found = false;
            for (size_t i = 0; i < ident.ascii_len; i++) {
                if (ident.ascii[i] == '$' || ident.ascii[i] == '.') {
                    print_str(rdm, ident.ascii, i);
                    ident.ascii += i;
                    ident.ascii_len -= i;
                    found = true;
                    break;
                }
            }
            if (!found) {
                break;
            }
        }
    }

    print_str(rdm, ident.ascii, ident.ascii_len);
}

static void demangle_legacy_path(struct rust_demangler *rdm) {
    bool first = true;

    while (1) {
        if (eat(rdm, 'E')) {
            // FIXME Maybe check if at end of symbol?
            return;
        }

        struct rust_mangled_ident name = parse_ident(rdm);

        if (!rdm->verbose && peek(rdm) == 'E' && is_rust_hash(name)) {
            // Skip printing the hash if verbose mode is disabled.
            eat(rdm, 'E');
            break;
        }

        if (!first) {
            PRINT("::");
        }
        first = false;

        print_legacy_ident(rdm, name);

        CHECK_OR(!rdm->errored, return);
    }
}

bool rust_demangle_with_callback(
    const char *whole_mangled_symbol, int flags,
    void (*callback)(const char *data, size_t len, void *opaque), void *opaque
) {
    struct rust_demangler rdm;

    rdm.sym = whole_mangled_symbol;
    rdm.sym_len = 0;

    rdm.callback_opaque = opaque;
    rdm.callback = callback;

    rdm.next = 0;
    rdm.errored = false;
    rdm.skipping_printing = false;
    rdm.verbose = (flags & RUST_DEMANGLE_FLAG_VERBOSE) != 0;
    rdm.version = -2; // Invalid version
    rdm.bound_lifetime_depth = 0;

    // Rust symbols always start with R, _R or __R for the v0 scheme or ZN, _ZN
    // or __ZN for the legacy scheme.
    if (strncmp(rdm.sym, "_R", 2) == 0) {
        rdm.sym += 2;
        rdm.version = 0; // v0
    } else if (rdm.sym[0] == 'R') {
        // On Windows, dbghelp strips leading underscores, so we accept "R..."
        // form too.
        rdm.sym += 1;
        rdm.version = 0; // v0
    } else if (strncmp(rdm.sym, "__R", 3) == 0) {
        // On OSX, symbols are prefixed with an extra _
        rdm.sym += 3;
        rdm.version = 0; // v0
    } else if (strncmp(rdm.sym, "_ZN", 3) == 0) {
        rdm.sym += 3;
        rdm.version = -1; // legacy
    } else if (strncmp(rdm.sym, "ZN", 2) == 0) {
        // On Windows, dbghelp strips leading underscores, so we accept "R..."
        // form too.
        rdm.sym += 2;
        rdm.version = -1; // legacy
    } else if (strncmp(rdm.sym, "__ZN", 4) == 0) {
        // On OSX, symbols are prefixed with an extra _
        rdm.sym += 4;
        rdm.version = -1; // legacy
    } else {
        return false;
    }

    if (rdm.version != -1) {
        // Paths always start with uppercase characters.
        if (!IS_UPPER(rdm.sym[0]))
            return false;
    }

    // Rust symbols only use ASCII characters.
    for (const char *p = rdm.sym; *p; p++) {
        if ((*p & 0x80) != 0)
            return false;

        if (*p == '.' && strncmp(p, ".llvm.", 6) == 0) {
            // Ignore .llvm.<hash> suffixes
            break;
        }

        rdm.sym_len++;
    }

    if (rdm.version == -1) {
        demangle_legacy_path(&rdm);
    } else {
        demangle_path(&rdm, true);

        // Skip instantiating crate.
        if (!rdm.errored && rdm.next < rdm.sym_len && peek(&rdm) >= 'A' &&
            peek(&rdm) <= 'Z') {
            rdm.skipping_printing = true;
            demangle_path(&rdm, false);
        }
    }

    if (!rdm.errored && (rdm.sym_len - rdm.next > 0)) {
        for (const char *p = rdm.sym + rdm.next; *p; p++) {
            // FIXME match is_symbol_like from rustc-demangle
            if (!((*p >= 'a' && *p <= 'z') || (*p >= 'A' && *p <= 'Z') ||
                  (*p >= '0' && *p <= '9') || *p == '.')) {
                // Suffix is not a symbol like string
                return false;
            }
        }

        // Print LLVM produced suffix
        print_str(&rdm, rdm.sym + rdm.next, rdm.sym_len - rdm.next);
    }

    return !rdm.errored;
}

// Growable string buffers.
struct str_buf {
    char *ptr;
    size_t len;
    size_t cap;
    bool errored;
};

static void str_buf_reserve(struct str_buf *buf, size_t extra) {
    // Allocation failed before.
    if (buf->errored)
        return;

    size_t available = buf->cap - buf->len;

    if (extra <= available)
        return;

    size_t min_new_cap = buf->cap + (extra - available);

    // Check for overflows.
    if (min_new_cap < buf->cap) {
        buf->errored = true;
        return;
    }

    size_t new_cap = buf->cap;

    if (new_cap == 0)
        new_cap = 4;

    // Double capacity until sufficiently large.
    while (new_cap < min_new_cap) {
        new_cap *= 2;

        // Check for overflows.
        if (new_cap < buf->cap) {
            buf->errored = true;
            return;
        }
    }

    char *new_ptr = (char *)realloc(buf->ptr, new_cap);
    if (new_ptr == NULL) {
        free(buf->ptr);
        buf->ptr = NULL;
        buf->len = 0;
        buf->cap = 0;
        buf->errored = true;
    } else {
        buf->ptr = new_ptr;
        buf->cap = new_cap;
    }
}

static void str_buf_append(struct str_buf *buf, const char *data, size_t len) {
    str_buf_reserve(buf, len);
    if (buf->errored)
        return;

    memcpy(buf->ptr + buf->len, data, len);
    buf->len += len;
}

static void
str_buf_demangle_callback(const char *data, size_t len, void *opaque) {
    str_buf_append(opaque, data, len);
}

char *rust_demangle(const char *mangled, int flags) {
    struct str_buf out;

    out.ptr = NULL;
    out.len = 0;
    out.cap = 0;
    out.errored = false;

    bool success = rust_demangle_with_callback(
        mangled, flags, str_buf_demangle_callback, &out
    );

    if (!success) {
        free(out.ptr);
        return NULL;
    }

    str_buf_append(&out, "\0", 1);
    return out.ptr;
}
