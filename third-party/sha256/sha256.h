// https://github.com/B-Con/crypto-algorithms/blob/master/sha256.c
//
// The original source file is in the public domain.

/*********************************************************************
* Filename:   sha256.h
* Author:     Brad Conte (brad AT bradconte.com)
* Copyright:
* Disclaimer: This code is presented "as is" without any guarantees.
* Details:    Defines the API for the corresponding SHA1 implementation.
*********************************************************************/

#pragma once

#include <cstdint>

typedef uint8_t u8;
typedef uint32_t u32;
typedef uint64_t u64;

#define SHA256_BLOCK_SIZE 32

typedef struct {
  u8 data[64];
  u32 datalen;
  u64 bitlen;
  u32 state[8];
} SHA256_CTX;

int SHA256_Init(SHA256_CTX *ctx);
int SHA256_Update(SHA256_CTX *ctx, const void *data, size_t len);
int SHA256_Final(uint8_t *hash, SHA256_CTX *ctx);
u8 *SHA256(const u8 *data, size_t len, u8 *hash);
