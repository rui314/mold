#pragma once

#ifdef __APPLE__
#  define COMMON_DIGEST_FOR_OPENSSL
#  include <CommonCrypto/CommonDigest.h>
#else
#  include <nettle/sha.h>

   typedef struct sha256_ctx SHA256_CTX;
#  define SHA256_SIZE SHA256_DIGEST_SIZE
#  define SHA256_Init sha256_init
#  define SHA256_Update(ctx, data, len) \
          sha256_update((ctx), (len), reinterpret_cast<const uint8_t*>(data))
#  define SHA256_Final(data, ctx) \
          sha256_digest((ctx), SHA256_DIGEST_SIZE, (data))
#endif
