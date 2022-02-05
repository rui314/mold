# Coding Guidelines

mold is written in C++20, but as is the case with every C++ project,
it has local coding rules. In this document, I'll explain some of them
and try to give justifications for why I chose such rules.

## DOs

- Always use `i64` (which is a type alias for `int64_t` in mold) for
  integers unless you have a reason not to. For example, even if you know
  that a loop counter won't exceed 100, you should stop thinking about it
  and just use `i64`.

  Justification: Local variables are usually on CPU registers, so on
  64-bit CPUs, there's no performance peanlty on choosing `i64` over
  `i32`. Even if a compiler has to spill register values to the stack,
  I don't think there's an observable difference between `i32` and
  `i64`. Therefore, extra 32 bits are essentially free. On 32-bit CPUs,
  they are not free, but that's OK because we are writing mold for modern
  computers. mold will still run on 32-bit computers but a bit slowly.

  By always using `i64`, we can eliminate the need to think about the
  "right" size for each variable. It also reduces the risk of integer
  overflow.

  Exceptions: If you have to allocate a very large number (e.g. millions)
  of the same object, its size matters. In that case, use a smaller type.

## DON'Ts

- Don't use `auto` unless its actual type is obvious in the very narrow
  context. Currently, we use `auto` only for lambdas.

  Justification: I think `auto` makes code writing easier but code reading
  harder, because readers have to make a guess as to what is the actual
  type of `auto`. If you are already familiar with the existing codebase,
  you may be able to guess it easily, but that's not always the case.
  I want to keep the mold codebase friendly to first-time readers.

- Don't over-use inheritance. In mold, most classes don't have parents,
  and even if they do, their class hierarchy is very shallow. Currently
  its height is just two (i.e. abstract classes and their implementations).

  Justification: Designing class hierarchies is fun as it feels like
  taxonomy, but I don't think that always help writing code. It looks like
  simpler class hierarchy makes its code simpler.
