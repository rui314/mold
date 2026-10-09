//! Native Windows LTO support.

use mold_common::fatal;
use mold_common::mapped_file::MappedFile;

use crate::arch::Target;
use crate::context::Context;
use crate::input_files::ObjectFile;

pub fn read_lto_object<E: Target>(
    _ctx: &Context<E>,
    _mf: &'static MappedFile,
    _archive_name: &'static std::path::Path,
) -> Option<ObjectFile<E>> {
    fatal!("LTO is not supported on Windows");
}

pub fn run_plugin<E: Target>(_ctx: &mut Context<E>) {}

pub fn cleanup() {}
