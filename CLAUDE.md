# CLAUDE.md

Code in this repository follows strict conventions. Read
`docs/coding-guidelines.md` if unfamiliar with any rule below.

## What is mold

mold is a fast drop-in replacement for GNU ld. It links ELF object files
into executables and shared libraries. It is a cross-linker — it can
target any architecture from any host. The man page source is
`docs/mold.md`; do not edit `docs/mold.1` (auto-generated).

## Build and Test

```bash
cmake -DCMAKE_BUILD_TYPE=Debug -DCMAKE_CXX_COMPILER=clang++ -B build
cmake --build build -j$(nproc)
cd build && ctest --output-on-failure -j$(nproc)
```

Quick smoke test — verify mold can link a trivial program:
```bash
echo 'int main() {}' | cc -B. -o /dev/null -xc -
```

## Code Style

- **Integers**: Use `i64` for everything. Use `i32` only when allocating
  millions of the same object. Use `ub32`/`ul32`/`il32` from
  `lib/integers.h` for file I/O fields (endian-safe, alignment-safe).
- **`auto`**: Use only for:
  - Lambdas: `auto fn = [&](i64 val) { ... };`
  - Structured bindings: `auto [it, inserted] = map.insert({key, val});`
  - Verbose types: `auto flags = std::regex_constants::optimize | ...;`
  Never for simple variable declarations — always write the explicit type.
- **`std::tie` vs `auto [x, y]`**: Use `std::tie` when the variables are
  already declared. Use `auto [x, y]` for new declarations.
- **`constexpr`**: Use `if constexpr` for template metaprogramming (target
  selection, arch-specific code paths). Don't add `constexpr` to functions
  unless required.
- **No exceptions**: The build uses `-fno-exceptions`. Use
  `Fatal(ctx)` / `Error(ctx)` / `Warn(ctx)` — RAII classes that print
  and exit in the destructor. Fatal calls `_exit(1)`.
- **No deep inheritance**: Class hierarchies are at most 2 levels deep.
  Prefer composition.
- **Variable names**: Short — `ctx` (Context), `mf` (MappedFile),
  `sym` (Symbol), `esym` (ElfSym), `loc` (relocation target address).
- **Helper lambdas**: Define locally for repeated operations within a
  function (e.g., `auto check = [&](i64 val, i64 lo, i64 hi) { ... };`
  for relocation range checks).
- **`append()`**: Use `append(vec, x)` from `lib/lib.h` instead of
  manual loops to append vectors.
- **Direct field access**: Prefer `isec.name` over `isec.name()` when
  the field is directly accessible. Remove unnecessary local variables
  that just alias a field.
- **Range-based for over index loop**: Prefer `for (Symbol<E> *sym : vec)`
  over `for (i64 i = 0; i < vec.size(); i++)` when the index isn't needed.
- **Semantic names over raw checks**: Prefer `ctx.gnu_debuglink` over
  `!ctx.arg.separate_debug_file.empty()`. Use meaningful boolean fields
  instead of checking argument emptiness.
- **Simplify**: The project values simplification — removing redundant
  code, replacing index loops with range-for, extracting complex lambdas
  into named functions, using semantic names. Simplification should be
  a standalone commit, not mixed with feature work.

## Architecture

- **Explicit template instantiation**: Every `.cc` file ends with
  `template class Foo<E>;` for the target type `E = MOLD_TARGET`.
  This keeps compile times fast. Don't use header-only templates.
- **`Context<E>` is the god object**: Almost every function takes
  `Context<E> &ctx`. It owns all linker state.
- **`namespace mold`**: All code lives here. No nested namespaces.
- **Include `mold.h`**: Most `.cc` files include only `mold.h`.
  Put `#include "mold.h"` first, then system headers.
- **ELF types**: Use types from `src/elf.h` (e.g. `Elf64_Sym`,
  `Elf64_Rela`). The template alias `U32<E>` selects endian-correct
  32-bit integers automatically.
- **Determinism**: mold's output must be bit-for-bit identical given the
  same inputs. No host-specific defaults. No non-deterministic iteration
  (hash tables, thread scheduling) in output-affecting code.
- **Linker scripts**: Intentionally minimal support — just enough to
  read `/usr/lib/x86_64-linux-gnu/libc.so`. No plans to add more.
- **Parallelism**: Uses TBB (`tbb::parallel_for_each`,
  `tbb::parallel_sort`). Use simple `for` loops when parallelism isn't
  needed. Don't parallelize just because you can — measure first.
  Avoid per-operation overhead (e.g., hash table insert per relocation).
- **Comments**: Explain WHY, not WHAT. Use full sentences. Detailed
  comments for complex logic (e.g., symbol resolution, .eh_frame parsing).
  No section separator lines (`// ====...`, `// ----...`).

## Writing a Test

Tests are shell scripts in `test/`. Every test sources `common.inc`:

```bash
#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo = 42;
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
extern int foo;
int main() { return foo != 42; }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$t/exe
```

- `$t` is a temp directory auto-created by `common.inc`.
- `$CC` / `$CXX` are set by `common.inc`.
- Use `not` to test expected failures.
- Use `skip` for arch-specific exclusions (`[ $MACHINE = aarch64 ] && skip`).

## Commits

- Short imperative: "Fix overflow in relocation", "Add SHT_NOTE support".
- No conventional commit prefixes (no `feat:`, `fix:`, `chore:`).
- No WIP commits. No draft PRs.
- No non-functional / stylistic changes — these are explicitly rejected.
- One logical change per PR. The maintainer splits and refactors after merge.
- Separate concerns: don't mix adding a feature with changing output format.
- Commit messages explain the problem, root cause, and fix — especially
  for bug fixes. Reference issue numbers when applicable.

## What NOT to Do

- Don't use `auto` for simple variable declarations — only for lambdas,
  structured bindings, and verbose types.
- Don't introduce `std::ranges` or `std::views`.
- Don't add `constexpr` to functions that don't need it.
- Don't use exceptions or `throw`.
- Don't refactor existing code for style — only functional changes.
- Don't use `int`, `unsigned`, `int32_t`, `uint32_t` for local integers.
- Don't add CMake options (`--enable-foo` / `--disable-foo`).
- Don't edit `docs/mold.1` directly — it's auto-generated from `docs/mold.md`.
- Don't propose optimizations without measuring memory impact —
  optimizations where memory cost outweighs speed gain are reverted.
- Don't parallelize code unnecessarily — simple `for` loops are fine
  when parallelism isn't needed.
- Don't use per-operation overhead (e.g., hash table insert per
  relocation) — solutions that add O(n) allocations are rejected.
- Don't extract complex lambdas into standalone functions unless the
  lambda is too complex to read inline.
