use crate::*;

pub const TEST_KEY: CVBytes = *b"whats the Elvish word for friend";

// Test a few different initial counter values.
// - 0: The base case.
// - i32::MAX: *No* overflow. But carry bugs in tricky SIMD code can screw this up, if you XOR when
//   you're supposed to ANDNOT.
// - u32::MAX: The low word of the counter overflows for all inputs except the first.
// - (42 << 32) + u32::MAX: Same but with a non-zero value in the high word.
const INITIAL_COUNTERS: [u64; 4] = [
    0,
    i32::MAX as u64,
    u32::MAX as u64,
    (42u64 << 32) + u32::MAX as u64,
];

const BLOCK_LENGTHS: [usize; 4] = [0, 1, 63, 64];

pub fn paint_test_input(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }
}

pub fn test_compress_vs_portable(test_impl: &Implementation) {
    for block_len in BLOCK_LENGTHS {
        dbg!(block_len);
        let mut block = [0; BLOCK_LEN];
        paint_test_input(&mut block[..block_len]);
        for counter in INITIAL_COUNTERS {
            dbg!(counter);
            let portable_cv = portable::implementation().compress(
                &block,
                block_len as u32,
                &TEST_KEY,
                counter,
                KEYED_HASH,
            );

            let test_cv =
                test_impl.compress(&block, block_len as u32, &TEST_KEY, counter, KEYED_HASH);

            assert_eq!(portable_cv, test_cv);
        }
    }
}

pub fn test_compress_vs_reference(test_impl: &Implementation) {
    for block_len in BLOCK_LENGTHS {
        dbg!(block_len);
        let mut block = [0; BLOCK_LEN];
        paint_test_input(&mut block[..block_len]);

        let mut ref_hasher = reference_impl::Hasher::new_keyed(&TEST_KEY);
        ref_hasher.update(&block[..block_len]);
        let mut ref_hash = [0u8; 32];
        ref_hasher.finalize(&mut ref_hash);

        let test_cv = test_impl.compress(
            &block,
            block_len as u32,
            &TEST_KEY,
            0,
            CHUNK_START | CHUNK_END | ROOT | KEYED_HASH,
        );

        assert_eq!(ref_hash, test_cv);
    }
}

fn check_transposed_eq(output_a: &TransposedVectors, output_b: &TransposedVectors) {
    if output_a == output_b {
        return;
    }
    for cv_index in 0..2 * MAX_SIMD_DEGREE {
        let cv_a = output_a.extract_cv(cv_index);
        let cv_b = output_b.extract_cv(cv_index);
        if cv_a == [0; 32] && cv_b == [0; 32] {
            println!("CV {cv_index:2} empty");
        } else if cv_a == cv_b {
            println!("CV {cv_index:2} matches");
        } else {
            println!("CV {cv_index:2} mismatch:");
            println!("    {}", hex::encode(cv_a));
            println!("    {}", hex::encode(cv_b));
        }
    }
    panic!("transposed outputs are not equal");
}

pub fn test_hash_chunks_vs_portable(test_impl: &Implementation) {
    assert!(test_impl.degree() <= MAX_SIMD_DEGREE);
    dbg!(test_impl.degree() * CHUNK_LEN);
    // Allocate 4 extra bytes of padding so we can make aligned slices.
    let mut input_buf = [0u8; 2 * 2 * MAX_SIMD_DEGREE * CHUNK_LEN + 4];
    let mut input_slice = &mut input_buf[..];
    // Make sure the start of the input is word-aligned.
    while input_slice.as_ptr() as usize % 4 != 0 {
        input_slice = &mut input_slice[1..];
    }
    let (aligned_input, mut unaligned_input) =
        input_slice.split_at_mut(2 * MAX_SIMD_DEGREE * CHUNK_LEN);
    unaligned_input = &mut unaligned_input[1..][..2 * MAX_SIMD_DEGREE * CHUNK_LEN];
    assert_eq!(aligned_input.as_ptr() as usize % 4, 0);
    assert_eq!(unaligned_input.as_ptr() as usize % 4, 1);
    paint_test_input(aligned_input);
    paint_test_input(unaligned_input);
    // Try just below, equal to, and just above every whole number of chunks.
    let mut input_2_lengths = Vec::new();
    let mut next_len = 2 * CHUNK_LEN;
    loop {
        // 95 is one whole block plus one interesting part of another
        input_2_lengths.push(next_len - 95);
        input_2_lengths.push(next_len);
        if next_len == test_impl.degree() * CHUNK_LEN {
            break;
        }
        input_2_lengths.push(next_len + 95);
        next_len += CHUNK_LEN;
    }
    for input_2_len in input_2_lengths {
        dbg!(input_2_len);
        let aligned_input1 = &aligned_input[..test_impl.degree() * CHUNK_LEN];
        let aligned_input2 = &aligned_input[test_impl.degree() * CHUNK_LEN..][..input_2_len];
        let unaligned_input1 = &unaligned_input[..test_impl.degree() * CHUNK_LEN];
        let unaligned_input2 = &unaligned_input[test_impl.degree() * CHUNK_LEN..][..input_2_len];
        for initial_counter in INITIAL_COUNTERS {
            dbg!(initial_counter);
            // Make two calls, to test the output_column parameter.
            let mut portable_output = TransposedVectors::new();
            let (portable_left, portable_right) =
                test_impl.split_transposed_vectors(&mut portable_output);
            portable::implementation().hash_chunks(
                aligned_input1,
                &IV_BYTES,
                initial_counter,
                0,
                portable_left,
            );
            portable::implementation().hash_chunks(
                aligned_input2,
                &TEST_KEY,
                initial_counter + test_impl.degree() as u64,
                KEYED_HASH,
                portable_right,
            );

            let mut test_output = TransposedVectors::new();
            let (test_left, test_right) = test_impl.split_transposed_vectors(&mut test_output);
            test_impl.hash_chunks(aligned_input1, &IV_BYTES, initial_counter, 0, test_left);
            test_impl.hash_chunks(
                aligned_input2,
                &TEST_KEY,
                initial_counter + test_impl.degree() as u64,
                KEYED_HASH,
                test_right,
            );
            check_transposed_eq(&portable_output, &test_output);

            // Do the same thing with unaligned input.
            let mut unaligned_test_output = TransposedVectors::new();
            let (unaligned_left, unaligned_right) =
                test_impl.split_transposed_vectors(&mut unaligned_test_output);
            test_impl.hash_chunks(
                unaligned_input1,
                &IV_BYTES,
                initial_counter,
                0,
                unaligned_left,
            );
            test_impl.hash_chunks(
                unaligned_input2,
                &TEST_KEY,
                initial_counter + test_impl.degree() as u64,
                KEYED_HASH,
                unaligned_right,
            );
            check_transposed_eq(&portable_output, &unaligned_test_output);
        }
    }
}

fn painted_transposed_input() -> TransposedVectors {
    let mut vectors = TransposedVectors::new();
    let mut val = 0;
    for col in 0..2 * MAX_SIMD_DEGREE {
        for row in 0..8 {
            vectors.0[row][col] = val;
            val += 1;
        }
    }
    vectors
}

pub fn test_hash_parents_vs_portable(test_impl: &Implementation) {
    assert!(test_impl.degree() <= MAX_SIMD_DEGREE);
    let input = painted_transposed_input();
    for num_parents in 2..=(test_impl.degree() / 2) {
        dbg!(num_parents);
        let mut portable_output = TransposedVectors::new();
        let (portable_left, portable_right) =
            test_impl.split_transposed_vectors(&mut portable_output);
        portable::implementation().hash_parents(
            &input,
            2 * num_parents, // num_cvs
            &IV_BYTES,
            0,
            portable_left,
        );
        portable::implementation().hash_parents(
            &input,
            2 * num_parents, // num_cvs
            &TEST_KEY,
            KEYED_HASH,
            portable_right,
        );

        let mut test_output = TransposedVectors::new();
        let (test_left, test_right) = test_impl.split_transposed_vectors(&mut test_output);
        test_impl.hash_parents(
            &input,
            2 * num_parents, // num_cvs
            &IV_BYTES,
            0,
            test_left,
        );
        test_impl.hash_parents(
            &input,
            2 * num_parents, // num_cvs
            &TEST_KEY,
            KEYED_HASH,
            test_right,
        );

        check_transposed_eq(&portable_output, &test_output);
    }
}

fn hash_with_chunks_and_parents_recurse(
    test_impl: &Implementation,
    input: &[u8],
    counter: u64,
    output: TransposedSplit,
) -> usize {
    assert!(input.len() > 0);
    if input.len() <= test_impl.degree() * CHUNK_LEN {
        return test_impl.hash_chunks(input, &IV_BYTES, counter, 0, output);
    }
    let (left_input, right_input) = input.split_at(left_len(input.len()));
    let mut child_output = TransposedVectors::new();
    let (left_output, right_output) = test_impl.split_transposed_vectors(&mut child_output);
    let mut children =
        hash_with_chunks_and_parents_recurse(test_impl, left_input, counter, left_output);
    assert_eq!(children, test_impl.degree());
    children += hash_with_chunks_and_parents_recurse(
        test_impl,
        right_input,
        counter + (left_input.len() / CHUNK_LEN) as u64,
        right_output,
    );
    test_impl.hash_parents(&child_output, children, &IV_BYTES, PARENT, output)
}

// Note: This test implementation doesn't support the 1-chunk-or-less case.
fn root_hash_with_chunks_and_parents(test_impl: &Implementation, input: &[u8]) -> CVBytes {
    // TODO: handle the 1-chunk case?
    assert!(input.len() > CHUNK_LEN);
    let mut cvs = TransposedVectors::new();
    // The right half of these vectors are never used.
    let (cvs_left, _) = test_impl.split_transposed_vectors(&mut cvs);
    let mut num_cvs = hash_with_chunks_and_parents_recurse(test_impl, input, 0, cvs_left);
    while num_cvs > 2 {
        num_cvs = test_impl.reduce_parents(&mut cvs, num_cvs, &IV_BYTES, 0);
    }
    test_impl.compress(
        &cvs.extract_parent_node(0),
        BLOCK_LEN as u32,
        &IV_BYTES,
        0,
        PARENT | ROOT,
    )
}

pub fn test_chunks_and_parents_vs_reference(test_impl: &Implementation) {
    assert_eq!(test_impl.degree().count_ones(), 1, "power of 2");
    const MAX_INPUT_LEN: usize = 2 * MAX_SIMD_DEGREE * CHUNK_LEN;
    let mut input_buf = [0u8; MAX_INPUT_LEN];
    paint_test_input(&mut input_buf);
    // Try just below, equal to, and just above every whole number of chunks, except that
    // root_hash_with_chunks_and_parents doesn't support the 1-chunk-or-less case.
    let mut test_lengths = vec![CHUNK_LEN + 1];
    let mut next_len = 2 * CHUNK_LEN;
    loop {
        // 95 is one whole block plus one interesting part of another
        test_lengths.push(next_len - 95);
        test_lengths.push(next_len);
        if next_len == MAX_INPUT_LEN {
            break;
        }
        test_lengths.push(next_len + 95);
        next_len += CHUNK_LEN;
    }
    for test_len in test_lengths {
        dbg!(test_len);
        let input = &input_buf[..test_len];

        let mut ref_hasher = reference_impl::Hasher::new();
        ref_hasher.update(&input);
        let mut ref_hash = [0u8; 32];
        ref_hasher.finalize(&mut ref_hash);

        let test_hash = root_hash_with_chunks_and_parents(test_impl, input);

        assert_eq!(ref_hash, test_hash);
    }
}

pub fn test_xof_vs_portable(test_impl: &Implementation) {
    let flags = CHUNK_START | CHUNK_END | KEYED_HASH;
    for counter in INITIAL_COUNTERS {
        dbg!(counter);
        for input_len in [0, 1, BLOCK_LEN] {
            dbg!(input_len);
            let mut input_block = [0u8; BLOCK_LEN];
            for byte_index in 0..input_len {
                input_block[byte_index] = byte_index as u8 + 42;
            }
            // Try equal to and partway through every whole number of output blocks.
            const MAX_OUTPUT_LEN: usize = 2 * MAX_SIMD_DEGREE * BLOCK_LEN;
            let mut output_lengths = Vec::new();
            let mut next_len = 0;
            loop {
                output_lengths.push(next_len);
                if next_len == MAX_OUTPUT_LEN {
                    break;
                }
                output_lengths.push(next_len + 31);
                next_len += BLOCK_LEN;
            }
            for output_len in output_lengths {
                dbg!(output_len);
                let mut portable_output = [0xff; MAX_OUTPUT_LEN];
                portable::implementation().xof(
                    &input_block,
                    input_len as u32,
                    &TEST_KEY,
                    counter,
                    flags,
                    &mut portable_output[..output_len],
                );
                let mut test_output = [0xff; MAX_OUTPUT_LEN];
                test_impl.xof(
                    &input_block,
                    input_len as u32,
                    &TEST_KEY,
                    counter,
                    flags,
                    &mut test_output[..output_len],
                );
                assert_eq!(portable_output, test_output);

                // Double check that the implementation didn't overwrite.
                assert!(test_output[output_len..].iter().all(|&b| b == 0xff));

                // The first XOR cancels out the output.
                test_impl.xof_xor(
                    &input_block,
                    input_len as u32,
                    &TEST_KEY,
                    counter,
                    flags,
                    &mut test_output[..output_len],
                );
                assert!(test_output[..output_len].iter().all(|&b| b == 0));
                assert!(test_output[output_len..].iter().all(|&b| b == 0xff));

                // The second XOR restores out the output.
                test_impl.xof_xor(
                    &input_block,
                    input_len as u32,
                    &TEST_KEY,
                    counter,
                    flags,
                    &mut test_output[..output_len],
                );
                assert_eq!(portable_output, test_output);
                assert!(test_output[output_len..].iter().all(|&b| b == 0xff));
            }
        }
    }
}

pub fn test_xof_vs_reference(test_impl: &Implementation) {
    let input = b"hello world";
    let mut input_block = [0; BLOCK_LEN];
    input_block[..input.len()].copy_from_slice(input);

    const MAX_OUTPUT_LEN: usize = 2 * MAX_SIMD_DEGREE * BLOCK_LEN;
    let mut ref_output = [0; MAX_OUTPUT_LEN];
    let mut ref_hasher = reference_impl::Hasher::new_keyed(&TEST_KEY);
    ref_hasher.update(input);
    ref_hasher.finalize(&mut ref_output);

    // Try equal to and partway through every whole number of output blocks.
    let mut output_lengths = vec![0, 1, 31];
    let mut next_len = BLOCK_LEN;
    loop {
        output_lengths.push(next_len);
        if next_len == MAX_OUTPUT_LEN {
            break;
        }
        output_lengths.push(next_len + 31);
        next_len += BLOCK_LEN;
    }

    for output_len in output_lengths {
        dbg!(output_len);
        let mut test_output = [0; MAX_OUTPUT_LEN];
        test_impl.xof(
            &input_block,
            input.len() as u32,
            &TEST_KEY,
            0,
            KEYED_HASH | CHUNK_START | CHUNK_END,
            &mut test_output[..output_len],
        );
        assert_eq!(ref_output[..output_len], test_output[..output_len]);

        // Double check that the implementation didn't overwrite.
        assert!(test_output[output_len..].iter().all(|&b| b == 0));

        // Do it again starting from block 1.
        if output_len >= BLOCK_LEN {
            test_impl.xof(
                &input_block,
                input.len() as u32,
                &TEST_KEY,
                1,
                KEYED_HASH | CHUNK_START | CHUNK_END,
                &mut test_output[..output_len - BLOCK_LEN],
            );
            assert_eq!(
                ref_output[BLOCK_LEN..output_len],
                test_output[..output_len - BLOCK_LEN],
            );
        }
    }
}

pub fn test_universal_hash_vs_portable(test_impl: &Implementation) {
    const MAX_INPUT_LEN: usize = 2 * MAX_SIMD_DEGREE * BLOCK_LEN;
    let mut input_buf = [0; MAX_INPUT_LEN];
    paint_test_input(&mut input_buf);
    // Try equal to and partway through every whole number of input blocks.
    let mut input_lengths = vec![0, 1, 31];
    let mut next_len = BLOCK_LEN;
    loop {
        input_lengths.push(next_len);
        if next_len == MAX_INPUT_LEN {
            break;
        }
        input_lengths.push(next_len + 31);
        next_len += BLOCK_LEN;
    }
    for input_len in input_lengths {
        dbg!(input_len);
        for counter in INITIAL_COUNTERS {
            dbg!(counter);
            let portable_output = portable::implementation().universal_hash(
                &input_buf[..input_len],
                &TEST_KEY,
                counter,
            );
            let test_output = test_impl.universal_hash(&input_buf[..input_len], &TEST_KEY, counter);
            assert_eq!(portable_output, test_output);
        }
    }
}

fn reference_impl_universal_hash(input: &[u8], key: &CVBytes) -> [u8; UNIVERSAL_HASH_LEN] {
    // The reference_impl doesn't support XOF seeking, so we have to materialize an entire extended
    // output to seek to a block.
    const MAX_BLOCKS: usize = 2 * MAX_SIMD_DEGREE;
    assert!(input.len() / BLOCK_LEN <= MAX_BLOCKS);
    let mut output_buffer: [u8; BLOCK_LEN * MAX_BLOCKS] = [0u8; BLOCK_LEN * MAX_BLOCKS];
    let mut result = [0u8; UNIVERSAL_HASH_LEN];
    let mut block_start = 0;
    while block_start < input.len() {
        let block_len = cmp::min(input.len() - block_start, BLOCK_LEN);
        let mut ref_hasher = reference_impl::Hasher::new_keyed(key);
        ref_hasher.update(&input[block_start..block_start + block_len]);
        ref_hasher.finalize(&mut output_buffer[..block_start + UNIVERSAL_HASH_LEN]);
        for byte_index in 0..UNIVERSAL_HASH_LEN {
            result[byte_index] ^= output_buffer[block_start + byte_index];
        }
        block_start += BLOCK_LEN;
    }
    result
}

pub fn test_universal_hash_vs_reference(test_impl: &Implementation) {
    const MAX_INPUT_LEN: usize = 2 * MAX_SIMD_DEGREE * BLOCK_LEN;
    let mut input_buf = [0; MAX_INPUT_LEN];
    paint_test_input(&mut input_buf);
    // Try equal to and partway through every whole number of input blocks.
    let mut input_lengths = vec![0, 1, 31];
    let mut next_len = BLOCK_LEN;
    loop {
        input_lengths.push(next_len);
        if next_len == MAX_INPUT_LEN {
            break;
        }
        input_lengths.push(next_len + 31);
        next_len += BLOCK_LEN;
    }
    for input_len in input_lengths {
        dbg!(input_len);
        let ref_output = reference_impl_universal_hash(&input_buf[..input_len], &TEST_KEY);
        let test_output = test_impl.universal_hash(&input_buf[..input_len], &TEST_KEY, 0);
        assert_eq!(ref_output, test_output);
    }
}
