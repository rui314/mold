//! The multi-threading abstractions used by `Hasher::update_with_join`.
//!
//! Different implementations of the `Join` trait determine whether
//! `Hasher::update_with_join` performs multi-threading on sufficiently large
//! inputs. The `SerialJoin` implementation is single-threaded, and the
//! `RayonJoin` implementation (gated by the `rayon` feature) is multi-threaded.
//! Interfaces other than `Hasher::update_with_join`, like [`hash`](crate::hash)
//! and [`Hasher::update`](crate::Hasher::update), always use `SerialJoin`
//! internally.
//!
//! The `Join` trait is an almost exact copy of the [`rayon::join`] API, and
//! `RayonJoin` is the only non-trivial implementation. Previously this trait
//! was public, but currently it's been re-privatized, as it's both 1) of no
//! value to most callers and 2) a pretty big implementation detail to commit
//! to.
//!
//! [`rayon::join`]: https://docs.rs/rayon/1.3.0/rayon/fn.join.html

/// The trait that abstracts over single-threaded and multi-threaded recursion.
///
/// See the [`join` module docs](index.html) for more details.
pub trait Join {
    fn join<A, B, RA, RB>(oper_a: A, oper_b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send;
}

/// The trivial, serial implementation of `Join`. The left and right sides are
/// executed one after the other, on the calling thread. The standalone hashing
/// functions and the `Hasher::update` method use this implementation
/// internally.
///
/// See the [`join` module docs](index.html) for more details.
pub enum SerialJoin {}

impl Join for SerialJoin {
    #[inline]
    fn join<A, B, RA, RB>(oper_a: A, oper_b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send,
    {
        (oper_a(), oper_b())
    }
}

/// The Rayon-based implementation of `Join`. The left and right sides are
/// executed on the Rayon thread pool, potentially in parallel. This
/// implementation is gated by the `rayon` feature, which is off by default.
///
/// See the [`join` module docs](index.html) for more details.
#[cfg(feature = "rayon")]
pub enum RayonJoin {}

#[cfg(feature = "rayon")]
impl Join for RayonJoin {
    #[inline]
    fn join<A, B, RA, RB>(oper_a: A, oper_b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send,
    {
        rayon_core::join(oper_a, oper_b)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_serial_join() {
        let oper_a = || 1 + 1;
        let oper_b = || 2 + 2;
        assert_eq!((2, 4), SerialJoin::join(oper_a, oper_b));
    }

    #[test]
    #[cfg(feature = "rayon")]
    fn test_rayon_join() {
        let oper_a = || 1 + 1;
        let oper_b = || 2 + 2;
        assert_eq!((2, 4), RayonJoin::join(oper_a, oper_b));
    }
}
