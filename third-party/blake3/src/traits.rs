//! Implementations of commonly used traits like `Digest` and `Mac` from the
//! [`digest`](https://crates.io/crates/digest) crate.

pub use digest;

use crate::{Hasher, OutputReader};
use digest::crypto_common;
use digest::generic_array::{typenum::U32, typenum::U64, GenericArray};

impl digest::HashMarker for Hasher {}

impl digest::Update for Hasher {
    #[inline]
    fn update(&mut self, data: &[u8]) {
        self.update(data);
    }
}

impl digest::Reset for Hasher {
    #[inline]
    fn reset(&mut self) {
        self.reset(); // the inherent method
    }
}

impl digest::OutputSizeUser for Hasher {
    type OutputSize = U32;
}

impl digest::FixedOutput for Hasher {
    #[inline]
    fn finalize_into(self, out: &mut GenericArray<u8, Self::OutputSize>) {
        out.copy_from_slice(self.finalize().as_bytes());
    }
}

impl digest::FixedOutputReset for Hasher {
    #[inline]
    fn finalize_into_reset(&mut self, out: &mut GenericArray<u8, Self::OutputSize>) {
        out.copy_from_slice(self.finalize().as_bytes());
        self.reset();
    }
}

impl digest::ExtendableOutput for Hasher {
    type Reader = OutputReader;

    #[inline]
    fn finalize_xof(self) -> Self::Reader {
        Hasher::finalize_xof(&self)
    }
}

impl digest::ExtendableOutputReset for Hasher {
    #[inline]
    fn finalize_xof_reset(&mut self) -> Self::Reader {
        let reader = Hasher::finalize_xof(self);
        self.reset();
        reader
    }
}

impl digest::XofReader for OutputReader {
    #[inline]
    fn read(&mut self, buffer: &mut [u8]) {
        self.fill(buffer);
    }
}

impl crypto_common::KeySizeUser for Hasher {
    type KeySize = U32;
}

impl crypto_common::BlockSizeUser for Hasher {
    type BlockSize = U64;
}

impl digest::MacMarker for Hasher {}

impl digest::KeyInit for Hasher {
    #[inline]
    fn new(key: &digest::Key<Self>) -> Self {
        let key_bytes: [u8; 32] = (*key).into();
        Hasher::new_keyed(&key_bytes)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_digest_traits() {
        // Inherent methods.
        let mut hasher1 = crate::Hasher::new();
        hasher1.update(b"foo");
        hasher1.update(b"bar");
        hasher1.update(b"baz");
        let out1 = hasher1.finalize();
        let mut xof1 = [0; 301];
        hasher1.finalize_xof().fill(&mut xof1);
        assert_eq!(out1.as_bytes(), &xof1[..32]);

        // Trait implementations.
        let mut hasher2: crate::Hasher = digest::Digest::new();
        digest::Digest::update(&mut hasher2, b"xxx");
        digest::Digest::reset(&mut hasher2);
        digest::Digest::update(&mut hasher2, b"foo");
        digest::Digest::update(&mut hasher2, b"bar");
        digest::Digest::update(&mut hasher2, b"baz");
        let out2 = digest::Digest::finalize(hasher2.clone());
        let mut xof2 = [0; 301];
        digest::XofReader::read(
            &mut digest::ExtendableOutput::finalize_xof(hasher2.clone()),
            &mut xof2,
        );
        assert_eq!(out1.as_bytes(), &out2[..]);
        assert_eq!(xof1[..], xof2[..]);

        // Again with the resetting variants.
        let mut hasher3: crate::Hasher = digest::Digest::new();
        digest::Digest::update(&mut hasher3, b"foobarbaz");
        let mut out3 = [0; 32];
        digest::FixedOutputReset::finalize_into_reset(
            &mut hasher3,
            GenericArray::from_mut_slice(&mut out3),
        );
        digest::Digest::update(&mut hasher3, b"foobarbaz");
        let mut out4 = [0; 32];
        digest::FixedOutputReset::finalize_into_reset(
            &mut hasher3,
            GenericArray::from_mut_slice(&mut out4),
        );
        digest::Digest::update(&mut hasher3, b"foobarbaz");
        let mut xof3 = [0; 301];
        digest::XofReader::read(
            &mut digest::ExtendableOutputReset::finalize_xof_reset(&mut hasher3),
            &mut xof3,
        );
        digest::Digest::update(&mut hasher3, b"foobarbaz");
        let mut xof4 = [0; 301];
        digest::XofReader::read(
            &mut digest::ExtendableOutputReset::finalize_xof_reset(&mut hasher3),
            &mut xof4,
        );
        assert_eq!(out1.as_bytes(), &out3[..]);
        assert_eq!(out1.as_bytes(), &out4[..]);
        assert_eq!(xof1[..], xof3[..]);
        assert_eq!(xof1[..], xof4[..]);
    }

    #[test]
    fn test_mac_trait() {
        // Inherent methods.
        let key = b"some super secret key bytes fooo";
        let mut hasher1 = crate::Hasher::new_keyed(key);
        hasher1.update(b"foo");
        hasher1.update(b"bar");
        hasher1.update(b"baz");
        let out1 = hasher1.finalize();

        // Trait implementation.
        let generic_key = (*key).into();
        let mut hasher2: crate::Hasher = digest::Mac::new(&generic_key);
        digest::Mac::update(&mut hasher2, b"xxx");
        digest::Mac::reset(&mut hasher2);
        digest::Mac::update(&mut hasher2, b"foo");
        digest::Mac::update(&mut hasher2, b"bar");
        digest::Mac::update(&mut hasher2, b"baz");
        let out2 = digest::Mac::finalize(hasher2);
        assert_eq!(out1.as_bytes(), out2.into_bytes().as_slice());
    }

    fn expected_hmac_blake3(key: &[u8], input: &[u8]) -> [u8; 32] {
        // See https://en.wikipedia.org/wiki/HMAC.
        let key_hash;
        let key_prime = if key.len() <= 64 {
            key
        } else {
            key_hash = *crate::hash(key).as_bytes();
            &key_hash
        };
        let mut ipad = [0x36; 64];
        let mut opad = [0x5c; 64];
        for i in 0..key_prime.len() {
            ipad[i] ^= key_prime[i];
            opad[i] ^= key_prime[i];
        }
        let mut inner_state = crate::Hasher::new();
        inner_state.update(&ipad);
        inner_state.update(input);
        let mut outer_state = crate::Hasher::new();
        outer_state.update(&opad);
        outer_state.update(inner_state.finalize().as_bytes());
        outer_state.finalize().into()
    }

    #[test]
    fn test_hmac_compatibility() {
        use hmac::{Mac, SimpleHmac};

        // Test a short key.
        let mut x = SimpleHmac::<Hasher>::new_from_slice(b"key").unwrap();
        hmac::digest::Update::update(&mut x, b"data");
        let output = x.finalize().into_bytes();
        assert_ne!(output.len(), 0);
        let expected = expected_hmac_blake3(b"key", b"data");
        assert_eq!(expected, output.as_ref());

        // Test a range of key and data lengths, particularly to exercise the long-key logic.
        let mut input_bytes = [0; crate::test::TEST_CASES_MAX];
        crate::test::paint_test_input(&mut input_bytes);
        for &input_len in crate::test::TEST_CASES {
            #[cfg(feature = "std")]
            dbg!(input_len);
            let input = &input_bytes[..input_len];

            let mut x = SimpleHmac::<Hasher>::new_from_slice(input).unwrap();
            hmac::digest::Update::update(&mut x, input);
            let output = x.finalize().into_bytes();
            assert_ne!(output.len(), 0);

            let expected = expected_hmac_blake3(input, input);
            assert_eq!(expected, output.as_ref());
        }
    }
}
