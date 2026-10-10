//! On macOS, a library is often distributed as a directory that holds the
//! library's .dylib together with the data files it uses at runtime, such
//! as images, UI layouts and translated messages. An application embeds
//! such directories for the libraries it uses, like this:
//!
//!   MyApp.app/Contents/Frameworks/Foo.framework/
//!     Foo
//!     Resources/icon.png
//!     Resources/MainMenu.nib
//!     Resources/ja.lproj/Localizable.strings
//!
//! Here, Foo is the library's .dylib file. In such a directory, the .dylib
//! is named after the directory, without the .dylib suffix.
//!
//! A library usually finds its data files by asking the Objective-C
//! runtime which file one of its classes was loaded from, with
//! class_getImageName() (which [NSBundle bundleForClass:] and Swift's
//! Bundle(for:) call), and looking in the Resources directory that goes
//! with that file, such as
//! MyApp.app/Contents/Frameworks/Foo.framework/Resources/ for
//! MyApp.app/Contents/Frameworks/Foo.framework/Foo. The runtime knows the
//! file, because it registers the classes of each executable and .dylib as
//! they are loaded. This works only as long as the library is linked
//! dynamically. If the library were linked statically, the runtime would
//! answer the application's executable, and the library would look in the
//! wrong directory. (A library written for static linking looks for its
//! data files by a name it knows instead, but many libraries aren't written
//! that way.)
//!
//! Linking libraries dynamically has a cost, though: an application starts
//! more slowly for each .dylib it has to load. So since Xcode 15, a library
//! can be built as a .dylib that can also be linked statically. Such a
//! .dylib keeps the relocations and other information of the object files
//! it was made from, so that the linker can turn it back into those object
//! files and link them into an executable as if they came from a static
//! library.
//!
//! Xcode links such a library statically into a release build of an
//! application. A debug build, which should be quick to link, instead links
//! an ordinary .dylib build of the library dynamically, and puts that .dylib
//! not in MyApp.app/Contents/Frameworks/Foo.framework/ but in a separate
//! directory, MyApp.app/Contents/Frameworks/ReexportedBinaries/Foo.framework/.
//! In both cases, the data files stay in
//! MyApp.app/Contents/Frameworks/Foo.framework/Resources/, but a library
//! written for dynamic linking looks for them elsewhere:
//!
//!  - In a release build, the runtime answers the application's executable,
//!    MyApp.app/Contents/MacOS/MyApp, so the library looks in the
//!    application's own MyApp.app/Contents/Resources/.
//!
//!  - In a debug build, the runtime answers
//!    MyApp.app/Contents/Frameworks/ReexportedBinaries/Foo.framework/Foo,
//!    so the library looks in
//!    MyApp.app/Contents/Frameworks/ReexportedBinaries/Foo.framework/Resources/,
//!    which has no data files.
//!
//! The macOS linker works around it by faking class_getImageName(). It adds
//! code to the output that, at startup, installs a function that answers
//! class_getImageName() in place of the runtime, with
//! objc_setHook_getImageName(). For the library's classes, the function
//! always answers MyApp.app/Contents/Frameworks/Foo.framework/Foo, whether
//! the library's code is in the application's executable or in
//! ReexportedBinaries, and whether or not that file is there. The library
//! then finds its data files. -no_merged_libraries_hook turns this off.
//!
//! The function needs a pointer to each class it answers for. In a debug
//! build, the application can refer to a class in the library's .dylib
//! only by an exported symbol, so the function in the application can
//! handle only the classes the library exports. With
//! -add_mergeable_debug_hook, the debug build of the library gets the
//! function too, for the rest of its classes.
//!
//! mold's version of the function is c/bundle-hook.c, which is compiled
//! ahead of time and embedded in mold. It finds the classes it answers
//! for, and the names of their libraries, in a table that mold creates at
//! link time (see create_class_table). Like the macOS linker, mold puts the
//! code's object file before the other input files, so that it installs the
//! function before other startup code may ask the runtime. (In an output
//! that lists its startup functions in the newer __init_offsets form, they
//! run in the reverse order, as with the macOS linker; see
//! passes::convert_init_offsets.)
//!
//! Apple calls a library's directory a "framework" and its data files
//! "resources". It calls a library that can be linked both ways
//! "mergeable", the two ways to link it "merging" and "re-exporting", and
//! the function a "hook", hence the names in this file.

use crate::arch::Target;
use crate::cmdline::Args;
use crate::context::Context;
use crate::filetype::{fat_slice, foreign_arch, is_subtype_mismatch};
use crate::input_files::{DataField, FileId, add_data_blob, read_dylib_binary, read_tbd};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::objc::ObjcRef;
use crate::symbol::SymbolId;
use crate::tapi::LdSymbols;

/// The hook, built by c/build-bundle-hook.sh.
static ARM64_OBJECT: &[u8] = include_bytes!("c/bundle-hook-arm64.o");
static X86_64_OBJECT: &[u8] = include_bytes!("c/bundle-hook-x86_64.o");

/// The table the hook reads, which mold defines.
const TABLE_SYMBOL: &[u8] = b"___mold_bundle_hook_table";

/// The classes the hook is for, by library.
#[derive(Default)]
pub struct BundleHook {
    /// The hook's object, if the link may need it.
    pub obj: Option<usize>,
    /// Each merged library that defines classes: its name, and the
    /// object its code makes (see reader::merge_dylib).
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
/// itself (see exported_classes), which the hook binds to in whichever
/// library loads them.
pub fn note_reexported_library<E: Target>(ctx: &mut Context<E>, mf: &'static MappedFile) {
    if !may_hook(&ctx.args) {
        return;
    }
    let Some((install_name, classes)) = exported_classes(ctx, mf) else {
        return;
    };
    if !classes.is_empty() {
        let ids = classes.into_iter().map(|name| ctx.symbols.intern(name)).collect();
        ctx.bundle_hook.reexported.push((leaf(&install_name), ids));
    }
}

/// A dylib's or stub's install name, and the Objective-C and Swift
/// classes (see is_class_export) it exports itself for the link's
/// target, after its $ld$hide and $ld$add directives: the classes of
/// the libraries it re-exports don't count, those it re-exports one by
/// one (an alias, a -reexported_symbols_list entry) do. None for a file
/// the link ignores, or one that is no library. (ld-prime adds its hook
/// for such classes to an image that re-exports the library with
/// -no_merge_*.)
fn exported_classes<E: Target>(
    ctx: &Context<E>,
    mf: &'static MappedFile,
) -> Option<(Vec<u8>, Vec<&'static [u8]>)> {
    use crate::filetype::{FileType, get_file_type};
    let mf = match get_file_type(mf) {
        FileType::Fat => fat_slice::<E>(&ctx.args, mf)?,
        _ => mf,
    };
    let (install_name, ld, exports) = match get_file_type(mf) {
        FileType::Tapi => {
            let tbd = read_tbd(ctx, mf)?;
            let ld = LdSymbols::read(ctx, &tbd.ld_symbols);
            let exports = [tbd.exports, tbd.weak_exports, tbd.tlv_exports].concat();
            (tbd.install_name.to_vec(), ld, exports)
        }
        FileType::Dylib
            if foreign_arch::<E>(mf).is_none()
                || (ctx.args.allow_sub_type_mismatches && is_subtype_mismatch::<E>(mf)) =>
        {
            let dylib = read_dylib_binary(mf);
            (dylib.install_name, LdSymbols::read(ctx, &dylib.ld_symbols), dylib.exports)
        }
        _ => return None,
    };
    let own = exports.into_iter().filter(|name| ld.keeps(name)).chain(ld.added.iter().copied());
    // A binary names its exports in its symbol table and export trie.
    let mut classes: Vec<&[u8]> = own.filter(|name| is_class_export(name)).collect();
    classes.sort_unstable();
    classes.dedup();
    Some((install_name, classes))
}

/// Whether an export is that of a class, as ld-prime's hook for the
/// classes of mergeable libraries goes by its name: an Objective-C
/// class or metaclass object (_OBJC_CLASS_$_Foo, _OBJC_METACLASS_$_Foo),
/// or a Swift class's type metadata (_$s...CN, of any class, Objective-C
/// or not). A Swift class's other symbols (its nominal type descriptor,
/// metaclass or accessor), and an Objective-C class's exception type or
/// instance variables, don't count.
fn is_class_export(name: &[u8]) -> bool {
    name.starts_with(b"_OBJC_CLASS_$_")
        || name.starts_with(b"_OBJC_METACLASS_$_")
        || (name.starts_with(b"_$s") && name.ends_with(b"CN"))
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
        ctx.objs[obj].is_reachable = false;
        crate::passes::resolve_symbols(ctx);
        return;
    }
    let table = add_table(ctx, &libraries);
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    let id = ctx.symbols.intern(TABLE_SYMBOL);
    let sym = &mut ctx.symbols[id];
    sym.set_file(FileId::Obj(internal));
    sym.set_input_section(Some(table));
    sym.value = 0;
    sym.set_extern(true);
    sym.set_private_extern(true);
}

/// Adds the table to __DATA,__data, after the libraries' names: the
/// count of classes, then each one's record of c/bundle-hook.c's
/// struct entry. Returns its subsection.
fn add_table<E: Target>(ctx: &mut Context<E>, libraries: &[(Vec<u8>, Vec<ObjcRef>)]) -> u32 {
    let names = libraries.iter().flat_map(|(name, _)| [&name[..], b"\0"].concat()).collect();
    let names = add_data_blob(ctx, b"__data", S_REGULAR, vec![DataField::Bytes(names)]);
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
    add_data_blob(ctx, b"__data", S_REGULAR, fields)
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
            && ctx.objs[file].is_reachable
            && isec.is_alive()
            && isec.hdr(&ctx.objs[file]).sectname() == b"__objc_classlist"
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
