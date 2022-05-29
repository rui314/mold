#ifdef __APPLE__
#  define COMMON_DIGEST_FOR_OPENSSL
#  include <CommonCrypto/CommonDigest.h>
#  define SHA256(data, len, md) CC_SHA256(data, len, md)
#else
#  define OPENSSL_SUPPRESS_DEPRECATED 1
#  include <openssl/sha.h>
#endif
