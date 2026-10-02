//! The hook for the classes of mergeable libraries. Xcode keeps a
//! mergeable framework's bundle in the app, with its resources, where
//! an image merges the framework's code (-merge_*) or re-exports it
//! from elsewhere (-no_merge_*), and so +[NSBundle bundleForClass:]
//! would find the image's bundle for the framework's classes. ld-prime
//! links a hook into such an image, unless -no_merged_libraries_hook:
//! an initializer that, in an app, has the Objective-C runtime name the
//! framework's binary in the app as each class's image
//! (objc_setHook_getImageName), from which NSBundle finds the bundle.
//! A debug build of a mergeable dylib gets one for the classes it
//! doesn't export with -add_mergeable_debug_hook.
//!
//! mold's hook is a C file of its own, c/bundle-hook.c, built ahead of
//! time and embedded. It looks a class up in a table mold makes: a
//! count, then a record per class of the class (a rebase or a bind),
//! its library's name and the path the hook makes from it. ld-prime's
//! hook has tables of another layout, and links Foundation,
//! CoreFoundation, libc++ and libswiftCore; mold's needs libSystem.
//! The hook's object goes first, as ld-prime's does, which so runs its
//! initializer first from __mod_init_func, but last from __init_offsets
//! (see passes::convert_init_offsets).

use crate::cmdline::Args;
use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::objc::{DataField, ObjcRef};
use crate::symbol::SymbolId;
use crate::target::Target;

/// The hook, built by c/build-bundle-hook.sh.
static ARM64_OBJECT: &[u8] = include_bytes!("../c/bundle-hook-arm64.o");
static X86_64_OBJECT: &[u8] = include_bytes!("../c/bundle-hook-x86_64.o");

/// The table the hook reads, which mold defines.
const TABLE_SYMBOL: &[u8] = b"___mold_bundle_hook_table";

/// The classes the hook is for, by library.
#[derive(Default)]
pub struct BundleHook {
    /// The hook's object, if the link may need it.
    pub obj: Option<usize>,
    /// Each merged library that defines classes: its name, and the
    /// object its code makes (see passes::merge_dylib).
    merged: Vec<(Vec<u8>, &'static MappedFile)>,
    /// Each re-exported library that exports classes: its name, and
    /// the classes, which the table binds to.
    reexported: Vec<(Vec<u8>, Vec<SymbolId>)>,
}

impl BundleHook {
    /// The classes the table binds to, which resolution looks up as if
    /// something referred to them.
    pub fn imports(&self) -> impl Iterator<Item = SymbolId> + '_ {
        self.reexported.iter().flat_map(|(_, ids)| ids.iter().copied())
    }
}

/// Whether the output may get the hook: an image dyld loads, but none
/// -make_mergeable makes. (ld-prime adds the hook to that too, which a
/// later link then merges as any other code, its table symbols clashing
/// with those of the merging image's hook.)
fn may_hook(args: &Args) -> bool {
    args.merged_libraries_hook
        && !args.relocatable
        && !args.make_mergeable
        && matches!(args.output_type, MH_EXECUTE | MH_DYLIB | MH_BUNDLE)
        && args.links_dylibs()
}

/// A library's name in the hook: the last component of its install
/// name, which names its framework and the framework's binary.
fn leaf(install_name: &[u8]) -> Vec<u8> {
    install_name.rsplit(|&b| b == b'/').next().unwrap().to_vec()
}

/// Notes a merged library that defines classes, whose code makes the
/// object `obj`, for the hook.
pub fn note_merged_library<E: Target>(
    ctx: &mut Context<E>,
    install_name: &[u8],
    obj: &'static MappedFile,
) {
    if may_hook(&ctx.args) {
        ctx.bundle_hook.merged.push((leaf(install_name), obj));
    }
}

/// Notes the classes a library a -no_merge_* option names exports
/// itself (see input_files::exported_classes), which the hook binds to
/// in whichever library loads them.
pub fn note_reexported_library<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) {
    if !may_hook(&ctx.args) {
        return;
    }
    let Some((install_name, classes)) = crate::input_files::exported_classes(ctx, mf) else {
        return;
    };
    if !classes.is_empty() {
        let ids = classes.into_iter().map(|name| ctx.symbols.intern(name)).collect();
        ctx.bundle_hook.reexported.push((leaf(&install_name), ids));
    }
}

/// The hook's object, which goes ahead of the inputs as ld-prime's does,
/// if the link may need it: for a debug build's own classes, that only
/// resolving the symbols tells (see create_class_table).
pub fn hook_object<E: Target>(ctx: &Context<E>) -> Option<&'static MappedFile> {
    let hook = &ctx.bundle_hook;
    let debug = ctx.args.add_mergeable_debug_hook && may_hook(&ctx.args);
    if hook.merged.is_empty() && hook.reexported.is_empty() && !debug {
        return None;
    }
    let data = if E::CPUTYPE == CPU_TYPE_ARM64 { ARM64_OBJECT } else { X86_64_OBJECT };
    Some(MappedFile::synthesized("bundleForClassHook.o".into(), data.to_vec()))
}

/// Makes the hook's table, once the symbols are resolved, or drops the
/// hook if there is no class for it.
pub fn create_class_table<E: Target>(ctx: &mut Context<E>) {
    let Some(obj) = ctx.bundle_hook.obj else { return };
    let libraries = hooked_classes(ctx);
    if libraries.iter().all(|(_, classes)| classes.is_empty()) {
        // Resolving the symbols again unbinds what only the hook named.
        ctx.objs[obj].is_alive = false;
        crate::passes::resolve_symbols(ctx);
        crate::passes::keep_bitcode_imports(ctx);
        return;
    }
    let table = add_table(ctx, &libraries);
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    let id = ctx.symbols.intern(TABLE_SYMBOL);
    let sym = &mut ctx.symbols[id];
    sym.set_file(FileId::Obj(internal));
    sym.set_input_section(Some(table));
    sym.value = 0;
    sym.set_is_extern(true);
    sym.set_is_private_extern(true);
}

/// Adds the table to __DATA,__data, after the libraries' names: the
/// count of classes, then each one's record of c/bundle-hook.c's
/// struct entry. Returns its subsection.
fn add_table<E: Target>(ctx: &mut Context<E>, libraries: &[(Vec<u8>, Vec<ObjcRef>)]) -> u32 {
    let names = libraries.iter().flat_map(|(name, _)| [&name[..], b"\0"].concat()).collect();
    let names =
        crate::objc::add_data_blob(ctx, b"__data", S_REGULAR, vec![DataField::Bytes(names)]);
    let count: usize = libraries.iter().map(|(_, classes)| classes.len()).sum();
    let mut fields = vec![DataField::Bytes((count as u64).to_le_bytes().to_vec())];
    let mut name_off = 0;
    for (name, classes) in libraries {
        let name_ptr = DataField::Ptr(ObjcRef::Isec(names, name_off));
        for &cls in classes {
            fields.extend([DataField::Ptr(cls), name_ptr.clone(), DataField::Bytes(vec![0; 8])]);
        }
        name_off += name.len() as u64 + 1;
    }
    crate::objc::add_data_blob(ctx, b"__data", S_REGULAR, fields)
}

/// The classes the hook is for, by library, as the table lists them: a
/// merged library's, those its __objc_classlist lists (with no generic
/// Swift class, which has no metadata until made); a re-exported one's,
/// those it exports itself, whether classes or metaclasses, as dyld
/// binds them; with -add_mergeable_debug_hook, those of the image's
/// own class lists it doesn't export, under the image's name. (Where a
/// class comes twice, the hook takes the first.)
fn hooked_classes<E: Target>(ctx: &Context<E>) -> Vec<(Vec<u8>, Vec<ObjcRef>)> {
    let hook = &ctx.bundle_hook;
    let mut libraries = Vec::new();
    let mut merged = Vec::new();
    for (name, mf) in &hook.merged {
        let Some(obj) = ctx.objs.iter().position(|obj| std::ptr::eq(obj.mf, *mf)) else {
            continue;
        };
        merged.push(obj);
        libraries.push((name.clone(), listed_classes(ctx, |i| i == obj)));
    }
    for (name, ids) in &hook.reexported {
        let defined = ids.iter().filter(|&&id| ctx.symbols[id].is_defined());
        libraries.push((name.clone(), defined.map(|&id| ObjcRef::Sym(id, 0)).collect()));
    }
    if ctx.args.add_mergeable_debug_hook {
        let mut classes = listed_classes(ctx, |i| !merged.contains(&i));
        classes.retain(|&cls| !exports_class(ctx, cls));
        libraries.push((leaf(ctx.args.output_install_name()), classes));
    }
    libraries
}

/// The classes the __objc_classlist sections of the live objects `of`
/// picks list.
fn listed_classes<E: Target>(ctx: &Context<E>, of: impl Fn(usize) -> bool) -> Vec<ObjcRef> {
    let lists = (0..ctx.isecs.len() as u32).filter(|&i| {
        let isec = &ctx.isecs[i];
        let file = isec.file as usize;
        of(file)
            && ctx.objs[file].is_alive
            && isec.is_alive()
            && ctx.hdr_of(isec).sectname() == b"__objc_classlist"
    });
    lists.flat_map(|i| crate::objc::list_entries(ctx, i).flatten()).collect()
}

/// Whether the image exports a class of its own, by the symbol its
/// class list names it by, under the export options the export passes
/// are yet to apply (see passes::hide_all_exports and the like).
fn exports_class<E: Target>(ctx: &Context<E>, cls: ObjcRef) -> bool {
    let ObjcRef::Sym(id, 0) = cls else { return false };
    let sym = &ctx.symbols[id];
    let name = sym.name();
    sym.is_extern()
        && !sym.is_private_extern()
        && !ctx.args.no_exported_symbols
        && ctx.args.exported_symbols.as_ref().is_none_or(|list| list.find(name) != -1)
        && ctx.args.unexported_symbols.find(name) == -1
}

#[cfg(test)]
mod tests {
    use sha2::Digest;

    /// The embedded objects come from c/bundle-hook.c as it is: the
    /// hash build-bundle-hook.sh notes is the source's.
    #[test]
    fn objects_are_up_to_date() {
        let source = include_bytes!("../c/bundle-hook.c");
        let noted = include_str!("../c/bundle-hook.c.sha256");
        let hash: String =
            sha2::Sha256::digest(source).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(noted.split_whitespace().next(), Some(hash.as_str()));
    }
}
