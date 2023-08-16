use blake3::guts::{BLOCK_LEN, CHUNK_LEN};
use serde::{Deserialize, Serialize};

// A non-multiple of 4 is important, since one possible bug is to fail to emit
// partial words.
pub const OUTPUT_LEN: usize = 2 * BLOCK_LEN + 3;

pub const TEST_CASES: &[usize] = &[
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7,
    8,
    BLOCK_LEN - 1,
    BLOCK_LEN,
    BLOCK_LEN + 1,
    2 * BLOCK_LEN - 1,
    2 * BLOCK_LEN,
    2 * BLOCK_LEN + 1,
    CHUNK_LEN - 1,
    CHUNK_LEN,
    CHUNK_LEN + 1,
    2 * CHUNK_LEN,
    2 * CHUNK_LEN + 1,
    3 * CHUNK_LEN,
    3 * CHUNK_LEN + 1,
    4 * CHUNK_LEN,
    4 * CHUNK_LEN + 1,
    5 * CHUNK_LEN,
    5 * CHUNK_LEN + 1,
    6 * CHUNK_LEN,
    6 * CHUNK_LEN + 1,
    7 * CHUNK_LEN,
    7 * CHUNK_LEN + 1,
    8 * CHUNK_LEN,
    8 * CHUNK_LEN + 1,
    16 * CHUNK_LEN,  // AVX512's bandwidth
    31 * CHUNK_LEN,  // 16 + 8 + 4 + 2 + 1
    100 * CHUNK_LEN, // subtrees larger than MAX_SIMD_DEGREE chunks
];

pub const TEST_KEY: &[u8; blake3::KEY_LEN] = b"whats the Elvish word for friend";
pub const TEST_CONTEXT: &str = "BLAKE3 2019-12-27 16:29:52 test vectors context";

const COMMENT: &str = r#"
Each test is an input length and three outputs, one for each of the hash,
keyed_hash, and derive_key modes. The input in each case is filled with a
repeating sequence of 251 bytes: 0, 1, 2, ..., 249, 250, 0, 1, ..., and so on.
The key used with keyed_hash is the 32-byte ASCII string "whats the Elvish word
for friend", also given in the `key` field below. The context string used with
derive_key is the ASCII string "BLAKE3 2019-12-27 16:29:52 test vectors
context", also given in the `context_string` field below. Outputs are encoded
as hexadecimal. Each case is an extended output, and implementations should
also check that the first 32 bytes match their default-length output.
"#;

// Paint the input with a repeating byte pattern. We use a cycle length of 251,
// because that's the largest prime number less than 256. This makes it
// unlikely to swapping any two adjacent input blocks or chunks will give the
// same answer.
pub fn paint_test_input(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Cases {
    pub _comment: String,
    pub key: String,
    pub context_string: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Case {
    pub input_len: usize,
    pub hash: String,
    pub keyed_hash: String,
    pub derive_key: String,
}

pub fn generate_json() -> String {
    let mut cases = Vec::new();
    for &input_len in TEST_CASES {
        let mut input = vec![0; input_len];
        paint_test_input(&mut input);

        let mut hash_out = [0; OUTPUT_LEN];
        blake3::Hasher::new()
            .update(&input)
            .finalize_xof()
            .fill(&mut hash_out);

        let mut keyed_hash_out = [0; OUTPUT_LEN];
        blake3::Hasher::new_keyed(TEST_KEY)
            .update(&input)
            .finalize_xof()
            .fill(&mut keyed_hash_out);

        let mut derive_key_out = [0; OUTPUT_LEN];
        blake3::Hasher::new_derive_key(TEST_CONTEXT)
            .update(&input)
            .finalize_xof()
            .fill(&mut derive_key_out);

        cases.push(Case {
            input_len,
            hash: hex::encode(&hash_out[..]),
            keyed_hash: hex::encode(&keyed_hash_out[..]),
            derive_key: hex::encode(&derive_key_out[..]),
        });
    }

    let mut json = serde_json::to_string_pretty(&Cases {
        _comment: COMMENT.trim().replace("\n", " "),
        key: std::str::from_utf8(TEST_KEY).unwrap().to_string(),
        context_string: TEST_CONTEXT.to_string(),
        cases,
    })
    .unwrap();

    // Add a trailing newline.
    json.push('\n');
    json
}

pub fn read_test_vectors_file() -> String {
    let test_vectors_file_path = "./test_vectors.json";
    std::fs::read_to_string(test_vectors_file_path).expect("failed to read test_vectors.json")
}

pub fn parse_test_cases() -> Cases {
    let json = read_test_vectors_file();
    serde_json::from_str(&json).expect("failed to parse test_vectors.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_reference_impl_all_at_once(
        key: &[u8; blake3::KEY_LEN],
        input: &[u8],
        expected_hash: &[u8],
        expected_keyed_hash: &[u8],
        expected_derive_key: &[u8],
    ) {
        let mut out = vec![0; expected_hash.len()];
        let mut hasher = reference_impl::Hasher::new();
        hasher.update(input);
        hasher.finalize(&mut out);
        assert_eq!(expected_hash, &out[..]);

        let mut out = vec![0; expected_keyed_hash.len()];
        let mut hasher = reference_impl::Hasher::new_keyed(key);
        hasher.update(input);
        hasher.finalize(&mut out);
        assert_eq!(expected_keyed_hash, &out[..]);

        let mut out = vec![0; expected_derive_key.len()];
        let mut hasher = reference_impl::Hasher::new_derive_key(TEST_CONTEXT);
        hasher.update(input);
        hasher.finalize(&mut out);
        assert_eq!(expected_derive_key, &out[..]);
    }

    fn test_reference_impl_one_at_a_time(
        key: &[u8; blake3::KEY_LEN],
        input: &[u8],
        expected_hash: &[u8],
        expected_keyed_hash: &[u8],
        expected_derive_key: &[u8],
    ) {
        let mut out = vec![0; expected_hash.len()];
        let mut hasher = reference_impl::Hasher::new();
        for &b in input {
            hasher.update(&[b]);
        }
        hasher.finalize(&mut out);
        assert_eq!(expected_hash, &out[..]);

        let mut out = vec![0; expected_keyed_hash.len()];
        let mut hasher = reference_impl::Hasher::new_keyed(key);
        for &b in input {
            hasher.update(&[b]);
        }
        hasher.finalize(&mut out);
        assert_eq!(expected_keyed_hash, &out[..]);

        let mut out = vec![0; expected_derive_key.len()];
        let mut hasher = reference_impl::Hasher::new_derive_key(TEST_CONTEXT);
        for &b in input {
            hasher.update(&[b]);
        }
        hasher.finalize(&mut out);
        assert_eq!(expected_derive_key, &out[..]);
    }

    fn test_incremental_all_at_once(
        key: &[u8; blake3::KEY_LEN],
        input: &[u8],
        expected_hash: &[u8],
        expected_keyed_hash: &[u8],
        expected_derive_key: &[u8],
    ) {
        let mut out = vec![0; expected_hash.len()];
        let mut hasher = blake3::Hasher::new();
        hasher.update(input);
        hasher.finalize_xof().fill(&mut out);
        assert_eq!(expected_hash, &out[..]);
        assert_eq!(&expected_hash[..32], hasher.finalize().as_bytes());

        let mut out = vec![0; expected_keyed_hash.len()];
        let mut hasher = blake3::Hasher::new_keyed(key);
        hasher.update(input);
        hasher.finalize_xof().fill(&mut out);
        assert_eq!(expected_keyed_hash, &out[..]);
        assert_eq!(&expected_keyed_hash[..32], hasher.finalize().as_bytes());

        let mut out = vec![0; expected_derive_key.len()];
        let mut hasher = blake3::Hasher::new_derive_key(TEST_CONTEXT);
        hasher.update(input);
        hasher.finalize_xof().fill(&mut out);
        assert_eq!(expected_derive_key, &out[..]);
        assert_eq!(&expected_derive_key[..32], hasher.finalize().as_bytes());
    }

    fn test_incremental_one_at_a_time(
        key: &[u8; blake3::KEY_LEN],
        input: &[u8],
        expected_hash: &[u8],
        expected_keyed_hash: &[u8],
        expected_derive_key: &[u8],
    ) {
        let mut out = vec![0; expected_hash.len()];
        let mut hasher = blake3::Hasher::new();
        for i in 0..input.len() {
            hasher.update(&[input[i]]);
            assert_eq!(i as u64 + 1, hasher.count());
        }
        hasher.finalize_xof().fill(&mut out);
        assert_eq!(expected_hash, &out[..]);
        assert_eq!(&expected_hash[..32], hasher.finalize().as_bytes());

        let mut out = vec![0; expected_keyed_hash.len()];
        let mut hasher = blake3::Hasher::new_keyed(key);
        for i in 0..input.len() {
            hasher.update(&[input[i]]);
            assert_eq!(i as u64 + 1, hasher.count());
        }
        hasher.finalize_xof().fill(&mut out);
        assert_eq!(expected_keyed_hash, &out[..]);
        assert_eq!(&expected_keyed_hash[..32], hasher.finalize().as_bytes());

        let mut out = vec![0; expected_derive_key.len()];
        let mut hasher = blake3::Hasher::new_derive_key(TEST_CONTEXT);
        for i in 0..input.len() {
            hasher.update(&[input[i]]);
            assert_eq!(i as u64 + 1, hasher.count());
        }
        hasher.finalize_xof().fill(&mut out);
        assert_eq!(expected_derive_key, &out[..]);
        assert_eq!(&expected_derive_key[..32], hasher.finalize().as_bytes());
    }

    fn test_recursive(
        key: &[u8; blake3::KEY_LEN],
        input: &[u8],
        expected_hash: &[u8],
        expected_keyed_hash: &[u8],
        expected_derive_key: &[u8],
    ) {
        assert_eq!(&expected_hash[..32], blake3::hash(input).as_bytes());
        assert_eq!(
            &expected_keyed_hash[..32],
            blake3::keyed_hash(key, input).as_bytes(),
        );
        assert_eq!(
            expected_derive_key[..32],
            blake3::derive_key(TEST_CONTEXT, input)
        );
    }

    #[test]
    fn run_test_vectors() {
        let cases = parse_test_cases();
        let key: &[u8; blake3::KEY_LEN] = cases.key.as_bytes().try_into().unwrap();
        for case in &cases.cases {
            dbg!(case.input_len);
            let mut input = vec![0; case.input_len];
            paint_test_input(&mut input);
            let expected_hash = hex::decode(&case.hash).unwrap();
            let expected_keyed_hash = hex::decode(&case.keyed_hash).unwrap();
            let expected_derive_key = hex::decode(&case.derive_key).unwrap();

            test_reference_impl_all_at_once(
                key,
                &input,
                &expected_hash,
                &expected_keyed_hash,
                &expected_derive_key,
            );

            test_reference_impl_one_at_a_time(
                key,
                &input,
                &expected_hash,
                &expected_keyed_hash,
                &expected_derive_key,
            );

            test_incremental_all_at_once(
                key,
                &input,
                &expected_hash,
                &expected_keyed_hash,
                &expected_derive_key,
            );

            test_incremental_one_at_a_time(
                key,
                &input,
                &expected_hash,
                &expected_keyed_hash,
                &expected_derive_key,
            );

            test_recursive(
                key,
                &input,
                &expected_hash,
                &expected_keyed_hash,
                &expected_derive_key,
            );
        }
    }

    #[test]
    fn test_checked_in_vectors_up_to_date() {
        // Replace Windows newlines, in case Git is configured to alter
        // newlines when files are checked out.
        let json = read_test_vectors_file().replace("\r\n", "\n");
        if generate_json() != json {
            panic!("Checked-in test_vectors.json is not up to date. Regenerate with `cargo run --bin generate > ./test_vectors.json`.");
        }
    }
}
