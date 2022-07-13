static constexpr int64_t SHA256_SIZE = 32;

#if defined(USE_VENDORERD_SHA256)
#  include "third-party/sha256/sha256.h"
#elif defined(_WIN32)
#include <Windows.h>
#include <bcrypt.h>
#include <ntstatus.h>

inline static std::once_flag sha256_alg_handle_flag;
inline static BCRYPT_ALG_HANDLE sha256_alg_handle() {
  static BCRYPT_ALG_HANDLE alg_handle;
  std::call_once(
      sha256_alg_handle_flag,
      [](BCRYPT_ALG_HANDLE* alg_handle_ptr) {
        BCryptOpenAlgorithmProvider(alg_handle_ptr, BCRYPT_SHA256_ALGORITHM,
                                    nullptr, 0);
      },
      &alg_handle);

  return alg_handle;
}
#elif defined(__APPLE__)
#  define COMMON_DIGEST_FOR_OPENSSL
#  include <CommonCrypto/CommonDigest.h>
#  define SHA256(data, len, md) CC_SHA256(data, len, md)
#else
#define OPENSSL_SUPPRESS_DEPRECATED 1
#include <openssl/sha.h>
#endif

inline void sha256_hash(unsigned char* input_begin, size_t input_size,
                 unsigned char* output_begin) {
#ifdef _WIN32
  BCryptHash(sha256_alg_handle(), nullptr, 0, input_begin, input_size,
             output_begin, SHA256_SIZE);
#else
  SHA256(input_begin, input_size, output_begin);
#endif
}

struct SHA256Hash {
#ifdef _WIN32
  BCRYPT_HASH_HANDLE hash_handle;
  DWORD hash_buffer_length, hash_object_length, data_length;
  std::unique_ptr<BYTE[]> hash_buffer, hash_object;
#else
  SHA256_CTX ctx;
#endif

  SHA256Hash() {
#ifdef _WIN32
    auto alg_handle = sha256_alg_handle();
    BCryptGetProperty(alg_handle, BCRYPT_OBJECT_LENGTH,
                      (PUCHAR)&hash_object_length, sizeof(DWORD), &data_length,
                      0);
    hash_object = std::make_unique<BYTE[]>(hash_object_length);
    BCryptGetProperty(alg_handle, BCRYPT_HASH_LENGTH,
                      (PUCHAR)&hash_buffer_length, sizeof(DWORD), &data_length,
                      0);
    hash_buffer = std::make_unique<BYTE[]>(hash_buffer_length);
    BCryptCreateHash(alg_handle, &hash_handle, hash_object.get(),
                     hash_object_length, nullptr, 0, 0);
#else
    SHA256_Init(&ctx);
#endif
  }

  void update(unsigned char* data, size_t size) {
#ifdef _WIN32
    BCryptHashData(sha256_alg_handle(), data, size, 0);
#else
    SHA256_Update(&ctx, data, size);
#endif
  }

  int finish(unsigned char* out) {
#ifdef _WIN32
    return BCryptFinishHash(sha256_alg_handle(), out, SHA256_SIZE, 0) == STATUS_SUCCESS;
#else
    return SHA256_Final(out, &ctx);
#endif
  }
};