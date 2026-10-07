use std::fs::File;
use std::io::Read;
use zerocopy::byteorder::little_endian::{U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes, Unaligned};

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct Header {
    magic: [u8; 8],
    version: U32,
    pub flags: U32,
    pub generation: U64,
    pub command: [u8; 32],
    pub output: [u8; 32],
    count: U32,
    reserved: U32,
    length: U64,
    checksum: [u8; 32],
}

#[derive(
    Clone, Copy, PartialEq, Eq, TryFromBytes, IntoBytes, KnownLayout, Immutable, Unaligned,
)]
#[repr(u8)]
pub(crate) enum EntityKind {
    Input = 0,
    ArchiveMember = 1,
    SearchResolution = 2,
    ToolchainEpoch = 3,
}

#[derive(Clone, Copy, TryFromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct Changed {
    pub index: U32,
    pub kind: U32,
    pub path: [u8; 32],
    pub old_identity: [u8; 64],
    pub digest: [u8; 32],
}

pub(crate) struct ChangeSet {
    pub header: Header,
    pub changed: Vec<Changed>,
    pub state_key: Option<[u8; 32]>,
}

impl ChangeSet {
    pub fn read() -> Option<Self> {
        let fd = std::env::var("MOLD_CHANGESET_FD").ok()?.parse::<i32>().ok()?;
        if !crate::mapped_file::sealed_fd(fd) {
            return None;
        }
        let mut file = File::open(format!("/proc/self/fd/{fd}")).ok()?;
        let length = usize::try_from(file.metadata().ok()?.len()).ok()?;
        if !(size_of::<Header>()..=1024 * 1024).contains(&length) {
            return None;
        }
        let mut data = vec![0; length];
        file.read_exact(&mut data).ok()?;
        Self::parse(&data)
    }

    fn parse(data: &[u8]) -> Option<Self> {
        let (header, extension) = Header::ref_from_prefix(data).ok()?;
        let (state_key, tail) = match header.version.get() {
            1 => (None, extension),
            2 => {
                let (key, tail) = <[u8; 32]>::ref_from_prefix(extension).ok()?;
                (Some(*key), tail)
            }
            _ => return None,
        };
        if header.magic != *b"MOLDCHG\0"
            || header.flags.get() & 1 == 0
            || header.flags.get() & !15 != 0
            || (header.flags.get() & 4 != 0 && state_key.is_none())
            || header.reserved.get() != 0
            || header.length.get() != data.len() as u64
            || (header.count.get() as usize).checked_mul(size_of::<Changed>())? != tail.len()
        {
            return None;
        }
        let mut hash = blake3::Hasher::new();
        hash.update(&data[..104]);
        hash.update(&[0; 32]);
        hash.update(extension);
        if hash.finalize().as_bytes() != &header.checksum {
            return None;
        }
        let changed = <[Changed]>::try_ref_from_bytes(tail).ok()?;
        let mut previous = None;
        for c in changed {
            if ![
                EntityKind::Input as u32,
                EntityKind::ArchiveMember as u32,
                EntityKind::SearchResolution as u32,
                EntityKind::ToolchainEpoch as u32,
            ]
            .contains(&c.kind.get())
                || (header.version.get() == 1 && c.kind.get() != EntityKind::Input as u32)
                || previous.is_some_and(|p| p >= (c.index.get(), c.kind.get(), c.path))
            {
                return None;
            }
            previous = Some((c.index.get(), c.kind.get(), c.path));
        }
        Some(Self { header: *header, changed: changed.to_vec(), state_key })
    }
}

const _: () = {
    assert!(size_of::<Header>() == 136);
    assert!(size_of::<Changed>() == 136);
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_command_contract_validation() {
        let mut header = Header {
            magic: *b"MOLDCHG\0",
            version: 2.into(),
            flags: 5.into(),
            generation: 1.into(),
            command: [1; 32],
            output: [2; 32],
            count: 0.into(),
            reserved: 0.into(),
            length: 168.into(),
            checksum: [0; 32],
        };
        let mut data = header.as_bytes().to_vec();
        data.extend_from_slice(&[3; 32]);
        header.checksum = *blake3::hash(&data).as_bytes();
        data[..136].copy_from_slice(header.as_bytes());
        let parsed = ChangeSet::parse(&data).unwrap();
        assert_eq!(parsed.state_key, Some([3; 32]));
        for offset in [0, 8, 12, 92, 96, 104, 136] {
            let mut invalid = data.clone();
            invalid[offset] ^= 255;
            assert!(ChangeSet::parse(&invalid).is_none());
        }
        assert!(ChangeSet::parse(&data[..136]).is_none());
        header.flags = 4.into();
        header.checksum = [0; 32];
        data[..136].copy_from_slice(header.as_bytes());
        header.checksum = *blake3::hash(&data).as_bytes();
        data[..136].copy_from_slice(header.as_bytes());
        assert!(ChangeSet::parse(&data).is_none());
    }
}
