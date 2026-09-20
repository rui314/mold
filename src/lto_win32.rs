//! Native Windows LTO support.

use crate::context::Context;
use crate::fatal;
use crate::input_files::ObjectFile;
use crate::mapped_file::MappedFile;
use crate::target::Target;

pub fn read_lto_object<E: Target>(
    _ctx: &mut Context<E>,
    _mf: &'static MappedFile,
    _archive_name: &'static std::path::Path,
) -> Option<ObjectFile<E>> {
    fatal!("LTO is not supported on Windows");
}

pub fn run_plugin<E: Target>(_ctx: &mut Context<E>) {}

pub fn cleanup() {}
