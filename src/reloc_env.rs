#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub(crate) enum RelocCell {
    Symbol = 0,
    Got = 1,
    GotTp = 2,
    TpOffset = 3,
    DtpOffset = 4,
    RelaxPredicate = 5,
}
