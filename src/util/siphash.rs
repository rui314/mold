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

    #[inline]
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

    #[inline(always)]
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

    #[inline(always)]
    fn compress(&mut self, m: u64) {
        self.v3 ^= m;
        for _ in 0..C_ROUNDS {
            self.round();
        }
        self.v0 ^= m;
    }

    #[inline(always)]
    fn finalize(&mut self) {
        for _ in 0..D_ROUNDS {
            self.round();
        }
    }
}

#[allow(dead_code)]
pub(crate) type SipHash = SipHashTmpl<2, 4, 64>;
#[allow(dead_code)]
pub(crate) type SipHash128 = SipHashTmpl<2, 4, 128>;
#[allow(dead_code)]
pub(crate) type SipHash13 = SipHashTmpl<1, 3, 64>;
#[allow(dead_code)]
pub(crate) type SipHash13_128 = SipHashTmpl<1, 3, 128>;
