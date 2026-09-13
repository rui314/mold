//! Native Windows LTO support.

use crate::arch::Arch;
use crate::context::Context;
use crate::fatal;
use crate::input_files::ObjectFile;
use crate::mapped_file::MappedFile;

pub fn read_lto_object<E: Arch>(
    _ctx: &mut Context<E>,
    _mf: &'static MappedFile,
    _archive_name: std::path::PathBuf,
) -> Option<ObjectFile<E>> {
    fatal!("LTO is not supported on Windows");
}

pub fn run_plugin<E: Arch>(_ctx: &mut Context<E>) {}

pub fn cleanup() {}
