use mold::arch::{I386, X86_64};
use mold::chunks::ChunkId;
use mold::cmdline::Args;
use mold::context::Context;

#[test]
fn output_chunk_survives_context_moves_and_chunk_reordering() {
    let mut ctx = Box::new(Context::<X86_64>::new(Args::default(), Vec::new()));
    let a = ctx.symbols.intern(b"first");
    let b = ctx.symbols.intern(b"second");
    ctx.set_symbol_output_chunk(a, ChunkId::Symtab);
    ctx.set_symbol_output_chunk(b, ChunkId::Got);
    ctx.symtab.hdr.shndx = 7;
    ctx.got.hdr.shndx = 11;
    ctx.chunks = vec![ChunkId::Symtab, ChunkId::Got];

    // Move the inline chunk headers to a different allocation.
    let mut moved = Box::new(*ctx);
    moved.chunks.reverse();
    assert_eq!(moved.symbols[a].output_chunk(&moved).unwrap().shndx, 7);
    assert_eq!(moved.symbols[b].output_chunk(&moved).unwrap().shndx, 11);

    moved.symtab.hdr.shndx = 13;
    assert_eq!(moved.symbols[a].output_chunk(&moved).unwrap().shndx, 13);
    moved.set_symbol_output_chunk(a, ChunkId::Got);
    assert_eq!(moved.symbols[a].output_chunk(&moved).unwrap().shndx, 11);
}

#[test]
fn output_chunk_uses_the_contexts_target_layout() {
    let mut ctx = Context::<I386>::new(Args::default(), Vec::new());
    let id = ctx.symbols.intern(b"synthetic");
    assert!(ctx.symbols[id].output_chunk(&ctx).is_none());
    ctx.set_symbol_output_chunk(id, ChunkId::Symtab);
    ctx.symtab.hdr.shdr.sh_addr.set(0x1234);
    assert_eq!(
        ctx.symbols[id]
            .output_chunk(&ctx)
            .unwrap()
            .shdr
            .sh_addr
            .get(),
        0x1234
    );
}
