#if defined(USE_VENDORERD_SHA256)
#  include "third-party/sha256/sha256.h"
#elif defined(_WIN32)
#elif defined(__APPLE__)
#  define COMMON_DIGEST_FOR_OPENSSL
#  include <CommonCrypto/CommonDigest.h>
#  define SHA256(data, len, md) CC_SHA256(data, len, md)
#else
#define OPENSSL_SUPPRESS_DEPRECATED 1
#include <openssl/sha.h>
#endif
