//! A configurable implementation of SipHash.
//!
//! The compression rounds, finalization rounds and 64- or 128-bit output are
//! selected with const generic parameters. This implementation is based on
//! the reference implementation at https://github.com/rui314/siphash.

pub(crate) struct SipHashTmpl<const C_ROUNDS: usize, const D_ROUNDS: usize, const OUTLEN: usize> {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    buf: [u8; 8],
    buflen: u8,
    sum: u8,
}

impl<const C_ROUNDS: usize, const D_ROUNDS: usize, const OUTLEN: usize>
    SipHashTmpl<C_ROUNDS, D_ROUNDS, OUTLEN>
{
    #[inline]
    pub(crate) fn new(key: &[u8; 16]) -> Self {
        assert!(OUTLEN == 64 || OUTLEN == 128);

        let k0 = u64::from_le_bytes(key[..8].try_into().unwrap());
        let k1 = u64::from_le_bytes(key[8..].try_into().unwrap());
        let mut hasher = Self {
            v0: 0x736f_6d65_7073_6575 ^ k0,
            v1: 0x646f_7261_6e64_6f6d ^ k1,
            v2: 0x6c79_6765_6e65_7261 ^ k0,
            v3: 0x7465_6462_7974_6573 ^ k1,
            buf: [0; 8],
            buflen: 0,
            sum: 0,
        };

        if OUTLEN == 128 {
            hasher.v1 ^= 0xee;
        }
        hasher
    }

    // ICF repeatedly hashes fixed-size digests. Inlining lets the optimizer
    // remove the buffering paths for these word-aligned updates.
    #[inline(always)]
    pub(crate) fn update(&mut self, mut msg: &[u8]) {
        self.sum = self.sum.wrapping_add(msg.len() as u8);

        if self.buflen != 0 {
            let buflen = self.buflen as usize;
            if buflen + msg.len() < 8 {
                self.buf[buflen..buflen + msg.len()].copy_from_slice(msg);
                self.buflen += msg.len() as u8;
                return;
            }

            let n = 8 - buflen;
            self.buf[buflen..].copy_from_slice(&msg[..n]);
            self.compress(u64::from_le_bytes(self.buf));
            msg = &msg[n..];
            self.buflen = 0;
        }

        while msg.len() >= 8 {
            self.compress(u64::from_le_bytes(msg[..8].try_into().unwrap()));
            msg = &msg[8..];
        }

        self.buf[..msg.len()].copy_from_slice(msg);
        self.buflen = msg.len() as u8;
    }

    /// Hashes a word in little-endian byte order. The common aligned case
    /// bypasses the byte buffer, including in loops of fixed-size updates.
    #[inline(always)]
    pub(crate) fn update_u64(&mut self, word: u64) {
        if self.buflen != 0 {
            self.update(&word.to_le_bytes());
            return;
        }
        self.sum = self.sum.wrapping_add(8);
        self.compress(word);
    }

    #[inline]
    pub(crate) fn finish(mut self, out: &mut [u8]) {
        assert_eq!(out.len() * 8, OUTLEN);

        self.buf[self.buflen as usize..].fill(0);
        self.compress((u64::from(self.sum) << 56) | u64::from_le_bytes(self.buf));

        self.v2 ^= if OUTLEN == 128 { 0xee } else { 0xff };
        self.finalize();
        out[..8].copy_from_slice(&(self.v0 ^ self.v1 ^ self.v2 ^ self.v3).to_le_bytes());

        if OUTLEN == 128 {
            self.v1 ^= 0xdd;
            self.finalize();
            out[8..].copy_from_slice(&(self.v0 ^ self.v1 ^ self.v2 ^ self.v3).to_le_bytes());
        }
    }

    #[inline]
    fn round(&mut self) {
        self.v0 = self.v0.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(13);
        self.v1 ^= self.v0;
        self.v0 = self.v0.rotate_left(32);
        self.v2 = self.v2.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(16);
        self.v3 ^= self.v2;
        self.v0 = self.v0.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(21);
        self.v3 ^= self.v0;
        self.v2 = self.v2.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(17);
        self.v1 ^= self.v2;
        self.v2 = self.v2.rotate_left(32);
    }

    #[inline]
    fn compress(&mut self, m: u64) {
        self.v3 ^= m;
        for _ in 0..C_ROUNDS {
            self.round();
        }
        self.v0 ^= m;
    }

    #[inline]
    fn finalize(&mut self) {
        for _ in 0..D_ROUNDS {
            self.round();
        }
    }
}

pub(crate) type SipHash13_128 = SipHashTmpl<1, 3, 128>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_updates_match_bytes_with_partial_buffers_and_length_wraparound() {
        let key = std::array::from_fn(|i| i as u8);
        let data: Vec<u8> = (0..1040).map(|i| (i * 97) as u8).collect();
        for prefix in 0..8 {
            for words in 0..128 {
                let end = prefix + words * 8;
                let mut bytes = SipHash13_128::new(&key);
                bytes.update(&data[..end + 3]);

                let mut mixed = SipHash13_128::new(&key);
                mixed.update(&data[..prefix]);
                for word in data[prefix..end].chunks_exact(8) {
                    mixed.update_u64(u64::from_le_bytes(word.try_into().unwrap()));
                }
                mixed.update(&data[end..end + 3]);
                let (mut expected, mut actual) = ([0; 16], [0; 16]);
                bytes.finish(&mut expected);
                mixed.finish(&mut actual);
                assert_eq!(actual, expected, "prefix={prefix}, words={words}");
            }
        }
    }
}
