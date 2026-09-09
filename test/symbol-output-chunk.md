
The reference cannot outlive its owning context:

```compile_fail,E0505
use mold::{arch::X86_64, chunks::ChunkId, cmdline::Args, context::Context};

let mut ctx = Context::<X86_64>::new(Args::default(), Vec::new());
let id = ctx.symbols.intern(b"synthetic");
ctx.set_symbol_output_chunk(id, ChunkId::Symtab);
let chunk = ctx.symbols[id].output_chunk(&ctx).unwrap();
drop(ctx);
println!("{}", chunk.shndx);
```

The context determines the chunk's architecture:

```compile_fail,E0308
use mold::{arch::{I386, X86_64}, chunks::ChunkId, cmdline::Args, context::Context};

let mut ctx = Context::<X86_64>::new(Args::default(), Vec::new());
let id = ctx.symbols.intern(b"synthetic");
ctx.set_symbol_output_chunk(id, ChunkId::Symtab);
let chunk = ctx.symbols[id].output_chunk::<I386>(&ctx);
```
