// This is a header-only C++20 implementation of SipHash based on the
// reference implementation. To use, just copy this header file into
// your project and #include it.
//
// https://github.com/rui314/siphash/blob/main/siphash.h

#include <bit>
#include <cstdint>
#include <cstring>

template <int C_ROUNDS, int D_ROUNDS, int OUTLEN>
class SipHashTmpl {
public:
  static_assert(OUTLEN == 64 || OUTLEN == 128);

  SipHashTmpl(void *key) {
    uint64_t k0 = read64(key);
    uint64_t k1 = read64((char *)key + 8);

    v0 = 0x736f6d6570736575 ^ k0;
    v1 = 0x646f72616e646f6d ^ k1;
    v2 = 0x6c7967656e657261 ^ k0;
    v3 = 0x7465646279746573 ^ k1;

    if (OUTLEN == 128)
      v1 ^= 0xee;
  }

  void update(void *msgp, int64_t msglen) {
    char *msg = (char *)msgp;
    sum += msglen;

    if (buflen) {
      if (buflen + msglen < 8) {
        memcpy(buf + buflen, msg, msglen);
        buflen += msglen;
        return;
      }

      int j = 8 - buflen;
      memcpy(buf + buflen, msg, j);
      compress(read64(buf));

      msg += j;
      msglen -= j;
      buflen = 0;
    }

    while (msglen >= 8) {
      compress(read64(msg));
      msg += 8;
      msglen -= 8;
    }

    memcpy(buf, msg, msglen);
    buflen = msglen;
  }

  void finish(void *out) {
    memset(buf + buflen, 0, 8 - buflen);
    compress(((uint64_t)sum << 56) | read64(buf));

    v2 ^= (OUTLEN == 128) ? 0xee : 0xff;
    finalize();
    write64(out, v0 ^ v1 ^ v2 ^ v3);

    if (OUTLEN == 128) {
      v1 ^= 0xdd;
      finalize();
      write64((char *)out + 8, v0 ^ v1 ^ v2 ^ v3);
    }
  }

  static void hash(void *out, void *key, void *in, int inlen) {
    SipHashTmpl<C_ROUNDS, D_ROUNDS, OUTLEN> h(key);
    h.update(in, inlen);
    h.finish(out);
  }

private:
  uint64_t v0, v1, v2, v3;
  uint8_t buf[8];
  uint8_t buflen = 0;
  uint8_t sum = 0;

  uint64_t read64(void *loc) {
    uint64_t val;
    memcpy(&val, loc, 8);
    if (std::endian::native == std::endian::big)
      val = bswap(val);
    return val;
  }

  void write64(void *loc, uint64_t val) {
    if (std::endian::native == std::endian::big)
      val = bswap(val);
    memcpy(loc, &val, 8);
  }

  uint64_t bswap(uint64_t val) {
    return ((val << 56) & 0xff00000000000000) |
           ((val << 40) & 0x00ff000000000000) |
           ((val << 24) & 0x0000ff0000000000) |
           ((val << 8)  & 0x000000ff00000000) |
           ((val >> 8)  & 0x00000000ff000000) |
           ((val >> 24) & 0x0000000000ff0000) |
           ((val >> 40) & 0x000000000000ff00) |
           ((val >> 56) & 0x00000000000000ff);
  }

  void round() {
    v0 += v1;
    v1 = std::rotl(v1, 13);
    v1 ^= v0;
    v0 = std::rotl(v0, 32);
    v2 += v3;
    v3 = std::rotl(v3, 16);
    v3 ^= v2;
    v0 += v3;
    v3 = std::rotl(v3, 21);
    v3 ^= v0;
    v2 += v1;
    v1 = std::rotl(v1, 17);
    v1 ^= v2;
    v2 = std::rotl(v2, 32);
  }

  void compress(uint64_t m) {
    v3 ^= m;
    for (int i = 0; i < C_ROUNDS; i++)
      round();
    v0 ^= m;
  }

  void finalize() {
    for (int i = 0; i < D_ROUNDS; i++)
      round();
  }
};

using SipHash = SipHashTmpl<2, 4, 64>;
using SipHash128 = SipHashTmpl<2, 4, 128>;
using SipHash13 = SipHashTmpl<1, 3, 64>;
using SipHash13_128 = SipHashTmpl<1, 3, 128>;
