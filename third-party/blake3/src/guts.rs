//! This undocumented and unstable module is for use cases like the `bao` crate,
//! which need to traverse the BLAKE3 Merkle tree and work with chunk and parent
//! chaining values directly. There might be breaking changes to this module
//! between patch versions.
//!
//! We could stabilize something like this module in the future. If you have a
//! use case for it, please let us know by filing a GitHub issue.

pub const BLOCK_LEN: usize = 64;
pub const CHUNK_LEN: usize = 1024;

#[derive(Clone, Debug)]
pub struct ChunkState(crate::ChunkState);

impl ChunkState {
    // Currently this type only supports the regular hash mode. If an
    // incremental user needs keyed_hash or derive_key, we can add that.
    pub fn new(chunk_counter: u64) -> Self {
        Self(crate::ChunkState::new(
            crate::IV,
            chunk_counter,
            0,
            crate::platform::Platform::detect(),
        ))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn update(&mut self, input: &[u8]) -> &mut Self {
        self.0.update(input);
        self
    }

    pub fn finalize(&self, is_root: bool) -> crate::Hash {
        let output = self.0.output();
        if is_root {
            output.root_hash()
        } else {
            output.chaining_value().into()
        }
    }
}

// As above, this currently assumes the regular hash mode. If an incremental
// user needs keyed_hash or derive_key, we can add that.
pub fn parent_cv(
    left_child: &crate::Hash,
    right_child: &crate::Hash,
    is_root: bool,
) -> crate::Hash {
    let output = crate::parent_node_output(
        left_child.as_bytes(),
        right_child.as_bytes(),
        crate::IV,
        0,
        crate::platform::Platform::detect(),
    );
    if is_root {
        output.root_hash()
    } else {
        output.chaining_value().into()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_chunk() {
        assert_eq!(
            crate::hash(b"foo"),
            ChunkState::new(0).update(b"foo").finalize(true)
        );
    }

    #[test]
    fn test_parents() {
        let mut hasher = crate::Hasher::new();
        let mut buf = [0; crate::CHUNK_LEN];

        buf[0] = 'a' as u8;
        hasher.update(&buf);
        let chunk0_cv = ChunkState::new(0).update(&buf).finalize(false);

        buf[0] = 'b' as u8;
        hasher.update(&buf);
        let chunk1_cv = ChunkState::new(1).update(&buf).finalize(false);

        hasher.update(b"c");
        let chunk2_cv = ChunkState::new(2).update(b"c").finalize(false);

        let parent = parent_cv(&chunk0_cv, &chunk1_cv, false);
        let root = parent_cv(&parent, &chunk2_cv, true);
        assert_eq!(hasher.finalize(), root);
    }
}
