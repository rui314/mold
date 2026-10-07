use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use zerocopy::byteorder::little_endian::U32;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct Cell {
    pub key: [u8; 32],
    pub kind: U32,
    pub properties: U32,
    pub begin: U32,
    pub capacity: U32,
}
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct SectionCells {
    pub begin: U32,
    pub count: U32,
}
pub(crate) type Key = ([u8; 32], u32);
pub(crate) type Tables = (Vec<Cell>, Vec<U32>, Vec<SectionCells>, Vec<U32>);
static TABLES: Mutex<Option<Tables>> = Mutex::new(None);
pub(crate) fn capture(sections: Vec<BTreeMap<Key, bool>>) {
    let mut reverse: BTreeMap<Key, (BTreeSet<u32>, bool)> = BTreeMap::new();
    for (index, cells) in sections.iter().enumerate() {
        for (cell, proof) in cells {
            let entry = reverse.entry(*cell).or_insert_with(|| (BTreeSet::new(), *proof));
            entry.0.insert(index as u32);
            entry.1 &= *proof;
        }
    }
    let mut cells = Vec::with_capacity(reverse.len());
    let mut consumers = Vec::new();
    let mut ids = BTreeMap::new();
    for ((key, kind), (values, proof)) in reverse {
        ids.insert((key, kind), cells.len() as u32);
        cells.push(Cell {
            key,
            kind: kind.into(),
            properties: u32::from(proof).into(),
            begin: (consumers.len() as u32).into(),
            capacity: (values.len() as u32 + 1).into(),
        });
        consumers.extend(values.into_iter().map(U32::new));
        consumers.push(U32::new(u32::MAX));
    }
    let mut edges = Vec::new();
    let mut links = Vec::with_capacity(sections.len());
    for section in sections {
        let mut next: Vec<_> = section.keys().map(|key| ids[key]).collect();
        next.sort_unstable();
        links.push(SectionCells {
            begin: (edges.len() as u32).into(),
            count: (next.len() as u32).into(),
        });
        edges.extend(next.into_iter().map(U32::new));
    }
    *TABLES.lock().unwrap() = Some((cells, consumers, links, edges));
}
pub(crate) fn take() -> Option<Tables> {
    TABLES.lock().unwrap().take()
}
const _: () = {
    assert!(size_of::<Cell>() == 48);
    assert!(size_of::<SectionCells>() == 8);
};
