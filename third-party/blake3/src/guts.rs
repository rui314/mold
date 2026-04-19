//! Deprecated in favor of [`hazmat`](crate::hazmat)

pub use crate::{BLOCK_LEN, CHUNK_LEN};

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
        self.0.count()
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
