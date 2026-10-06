//! mold's command line parsed with winnow-args (`--features winnow-args`).
//!
//! Every option is a variant of [`Item`], lexed by winnow-args with GNU ld's
//! dash rules (`long_only`) and kept in command-line order; `parse_args`
//! then folds the sequence through the same handling as the built-in parser.
//! `-z` keywords are mapped to the option they stand for by [`z_opt`].
//! Generated from `parse_args`'s option chain.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};

use winnow_args::value::Spanned;
use winnow_args::{Args, ErrorKind, Occurrence};

use mold_common::fatal;

#[derive(Occurrence)]
#[arg(allow_hyphen_values, keep_equals)]
pub(crate) enum Item {
    /// `-o`
    #[arg(short = 'o')]
    OutputShort(OsString),
    /// `--output`
    #[arg(long = "output", two_dashes)]
    Output(OsString),
    /// `-dynamic-linker`
    #[arg(long = "dynamic-linker")]
    DynamicLinker(OsString),
    /// `-I`
    #[arg(short = 'I')]
    DynamicLinkerShort(OsString),
    /// `-no-dynamic-linker`
    #[arg(long = "no-dynamic-linker")]
    NoDynamicLinker,
    /// `-v`
    #[arg(short = 'v')]
    ShortVLower,
    /// `-version`
    #[arg(long = "version")]
    Version,
    /// `-V`
    #[arg(short = 'V')]
    ShortV,
    /// `-mllvm`
    #[arg(long = "mllvm")]
    Mllvm(OsString),
    /// `-end-lib`
    #[arg(long = "end-lib")]
    EndLib,
    /// `-export-dynamic`
    #[arg(long = "export-dynamic")]
    ExportDynamic,
    /// `-E`
    #[arg(short = 'E')]
    ExportDynamicShort,
    /// `-no-export-dynamic`
    #[arg(long = "no-export-dynamic")]
    NoExportDynamic,
    /// `-Bsymbolic`
    #[arg(long = "Bsymbolic")]
    Bsymbolic,
    /// `-Bsymbolic-functions`
    #[arg(long = "Bsymbolic-functions")]
    BsymbolicFunctions,
    /// `-Bsymbolic-non-weak`
    #[arg(long = "Bsymbolic-non-weak")]
    BsymbolicNonWeak,
    /// `-Bsymbolic-non-weak-functions`
    #[arg(long = "Bsymbolic-non-weak-functions")]
    BsymbolicNonWeakFunctions,
    /// `-Bno-symbolic`
    #[arg(long = "Bno-symbolic")]
    BnoSymbolic,
    /// `-exclude-libs`
    #[arg(long = "exclude-libs")]
    ExcludeLibs(OsString),
    /// `-q`
    #[arg(short = 'q')]
    EmitRelocsShort,
    /// `-emit-relocs`
    #[arg(long = "emit-relocs")]
    EmitRelocs,
    /// `-Map`
    #[arg(long = "Map")]
    Map(OsString),
    /// `-print-dependencies`
    #[arg(long = "print-dependencies")]
    PrintDependencies,
    /// `-print-map`
    #[arg(long = "print-map")]
    PrintMap,
    /// `-M`
    #[arg(short = 'M')]
    PrintMapShort,
    /// `-Bstatic`
    #[arg(long = "Bstatic")]
    Bstatic,
    /// `-dn`
    #[arg(long = "dn")]
    Dn,
    /// `-static`
    #[arg(long = "static")]
    Static,
    /// `-Bdynamic`
    #[arg(long = "Bdynamic")]
    Bdynamic,
    /// `-dy`
    #[arg(long = "dy")]
    Dy,
    /// `-shared`
    #[arg(long = "shared")]
    Shared,
    /// `-Bshareable`
    #[arg(long = "Bshareable")]
    Bshareable,
    /// `-spare-dynamic-tags`
    #[arg(long = "spare-dynamic-tags")]
    SpareDynamicTags(OsString),
    /// `-spare-program-headers`
    #[arg(long = "spare-program-headers")]
    SpareProgramHeaders(OsString),
    /// `-start-lib`
    #[arg(long = "start-lib")]
    StartLib,
    /// `-start-stop`
    #[arg(long = "start-stop")]
    StartStop,
    /// `-dependency-file`
    #[arg(long = "dependency-file")]
    DependencyFile(OsString),
    /// `-defsym`
    #[arg(long = "defsym")]
    Defsym(OsString),
    /// `-:lto-pass2`
    #[arg(long = ":lto-pass2")]
    InternalLtoPass2,
    /// `-:ignore-ir-file`
    #[arg(long = ":ignore-ir-file")]
    InternalIgnoreIrFile(OsString),
    /// `-demangle`
    #[arg(long = "demangle")]
    Demangle,
    /// `-no-demangle`
    #[arg(long = "no-demangle")]
    NoDemangle,
    /// `-detach`
    #[arg(long = "detach")]
    Detach,
    /// `-no-detach`
    #[arg(long = "no-detach")]
    NoDetach,
    /// `-default-symver`
    #[arg(long = "default-symver")]
    DefaultSymver,
    /// `-noinhibit-exec`
    #[arg(long = "noinhibit-exec")]
    NoinhibitExec,
    /// `-shuffle-sections`
    #[arg(long = "shuffle-sections", require_equals, default_missing = "\0")]
    ShuffleSections(OsString),
    /// `-reverse-sections`
    #[arg(long = "reverse-sections")]
    ReverseSections,
    /// `-rosegment`
    #[arg(long = "rosegment")]
    Rosegment,
    /// `-no-rosegment`
    #[arg(long = "no-rosegment")]
    NoRosegment,
    /// `-y`
    #[arg(short = 'y')]
    TraceSymbolShort(OsString),
    /// `-trace-symbol`
    #[arg(long = "trace-symbol")]
    TraceSymbol(OsString),
    /// `-filler`
    #[arg(long = "filler")]
    Filler(OsString),
    /// `-L`
    #[arg(short = 'L')]
    LibraryPathShort(OsString),
    /// `-library-path`
    #[arg(long = "library-path")]
    LibraryPath(OsString),
    /// `-sysroot`
    #[arg(long = "sysroot")]
    Sysroot(OsString),
    /// `-unique`
    #[arg(long = "unique")]
    Unique(OsString),
    /// `-unresolved-symbols`
    #[arg(long = "unresolved-symbols")]
    UnresolvedSymbols(OsString),
    /// `--undefined-glob`
    #[arg(long = "undefined-glob", two_dashes)]
    UndefinedGlob(OsString),
    /// `-require-defined`
    #[arg(long = "require-defined")]
    RequireDefined(OsString),
    /// `-init`
    #[arg(long = "init")]
    Init(OsString),
    /// `-fini`
    #[arg(long = "fini")]
    Fini(OsString),
    /// `-hash-style`
    #[arg(long = "hash-style")]
    HashStyle(OsString),
    /// `-soname`
    #[arg(long = "soname")]
    Soname(OsString),
    /// `-h`
    #[arg(short = 'h')]
    SonameShort(OsString),
    /// `-audit`
    #[arg(long = "audit")]
    Audit(OsString),
    /// `-depaudit`
    #[arg(long = "depaudit")]
    Depaudit(OsString),
    /// `-P`
    #[arg(short = 'P')]
    DepauditShort(OsString),
    /// `-allow-multiple-definition`
    #[arg(long = "allow-multiple-definition")]
    AllowMultipleDefinition,
    /// `-apply-dynamic-relocs`
    #[arg(long = "apply-dynamic-relocs")]
    ApplyDynamicRelocs,
    /// `-no-apply-dynamic-relocs`
    #[arg(long = "no-apply-dynamic-relocs")]
    NoApplyDynamicRelocs,
    /// `-trace`
    #[arg(long = "trace")]
    Trace,
    /// `-eh-frame-hdr`
    #[arg(long = "eh-frame-hdr")]
    EhFrameHdr,
    /// `-no-eh-frame-hdr`
    #[arg(long = "no-eh-frame-hdr")]
    NoEhFrameHdr,
    /// `-pie`
    #[arg(long = "pie")]
    Pie,
    /// `-pic-executable`
    #[arg(long = "pic-executable")]
    PicExecutable,
    /// `-no-pie`
    #[arg(long = "no-pie")]
    NoPie,
    /// `-no-pic-executable`
    #[arg(long = "no-pic-executable")]
    NoPicExecutable,
    /// `-nopie`
    #[arg(long = "nopie")]
    Nopie,
    /// `-relax`
    #[arg(long = "relax")]
    Relax,
    /// `-no-relax`
    #[arg(long = "no-relax")]
    NoRelax,
    /// `-gdb-index`
    #[arg(long = "gdb-index")]
    GdbIndex,
    /// `-no-gdb-index`
    #[arg(long = "no-gdb-index")]
    NoGdbIndex,
    /// `-r`
    #[arg(short = 'r')]
    RelocatableShort,
    /// `-relocatable`
    #[arg(long = "relocatable")]
    Relocatable,
    /// `-relocatable-merge-sections`
    #[arg(long = "relocatable-merge-sections")]
    RelocatableMergeSections,
    /// `-perf`
    #[arg(long = "perf")]
    Perf,
    /// `-pack-dyn-relocs=android, -pack-dyn-relocs=android+relr, -pack-dyn-relocs=none, -pack-dyn-relocs=relr`
    #[arg(long = "pack-dyn-relocs", require_equals)]
    PackDynRelocs(OsString),
    /// `--use-android-relr-tags`
    #[arg(long = "use-android-relr-tags", two_dashes)]
    UseAndroidRelrTags,
    /// `-no-use-android-relr-tags`
    #[arg(long = "no-use-android-relr-tags")]
    NoUseAndroidRelrTags,
    /// `-package-metadata`
    #[arg(long = "package-metadata", require_equals)]
    PackageMetadata(OsString),
    /// `-stats`
    #[arg(long = "stats")]
    Stats,
    /// `-C`
    #[arg(short = 'C')]
    DirectoryShort(OsString),
    /// `-directory`
    #[arg(long = "directory")]
    Directory(OsString),
    /// `-chroot`
    #[arg(long = "chroot")]
    Chroot(OsString),
    /// `-color-diagnostics, -color-diagnostics=always, -color-diagnostics=auto, -color-diagnostics=never`
    #[arg(long = "color-diagnostics", require_equals, default_missing = "\0")]
    ColorDiagnostics(OsString),
    /// `-no-color-diagnostics`
    #[arg(long = "no-color-diagnostics")]
    NoColorDiagnostics,
    /// `-warn-common`
    #[arg(long = "warn-common")]
    WarnCommon,
    /// `-no-warn-common`
    #[arg(long = "no-warn-common")]
    NoWarnCommon,
    /// `-warn-once`
    #[arg(long = "warn-once")]
    IgnoredWarnOnce,
    /// `-warn-shared-textrel`
    #[arg(long = "warn-shared-textrel")]
    WarnSharedTextrel,
    /// `-warn-textrel`
    #[arg(long = "warn-textrel")]
    WarnTextrel,
    /// `-enable-new-dtags`
    #[arg(long = "enable-new-dtags")]
    EnableNewDtags,
    /// `-disable-new-dtags`
    #[arg(long = "disable-new-dtags")]
    DisableNewDtags,
    /// `--execute-only`
    #[arg(long = "execute-only", two_dashes)]
    ExecuteOnly,
    /// `-zero-to-bss`
    #[arg(long = "zero-to-bss")]
    ZeroToBss,
    /// `-compress-debug-sections`
    #[arg(long = "compress-debug-sections")]
    CompressDebugSections(OsString),
    /// `-wrap`
    #[arg(long = "wrap")]
    Wrap(OsString),
    /// `--omagic`
    #[arg(long = "omagic", two_dashes)]
    Omagic,
    /// `-N`
    #[arg(short = 'N')]
    OmagicShort,
    /// `--no-omagic`
    #[arg(long = "no-omagic", two_dashes)]
    NoOmagic,
    /// `--oformat`
    #[arg(long = "oformat", two_dashes)]
    Oformat(OsString),
    /// `-retain-symbols-file`
    #[arg(long = "retain-symbols-file")]
    RetainSymbolsFile(OsString),
    /// `-section-align`
    #[arg(long = "section-align")]
    SectionAlign(OsString),
    /// `-section-start`
    #[arg(long = "section-start")]
    SectionStart(OsString),
    /// `-section-order`
    #[arg(long = "section-order")]
    SectionOrder(OsString),
    /// `-Tbss`
    #[arg(long = "Tbss")]
    Tbss(OsString),
    /// `-Tdata`
    #[arg(long = "Tdata")]
    Tdata(OsString),
    /// `-Ttext`
    #[arg(long = "Ttext")]
    Ttext(OsString),
    /// `-Ttext-segment`
    #[arg(long = "Ttext-segment")]
    TtextSegment(OsString),
    /// `-repro`
    #[arg(long = "repro")]
    Repro,
    /// `-no-undefined`
    #[arg(long = "no-undefined")]
    NoUndefined,
    /// `-separate-debug-file`
    #[arg(long = "separate-debug-file", require_equals, default_missing = "\0")]
    SeparateDebugFile(OsString),
    /// `-no-separate-debug-file`
    #[arg(long = "no-separate-debug-file")]
    NoSeparateDebugFile,
    /// `-nmagic`
    #[arg(long = "nmagic")]
    Nmagic,
    /// `-no-nmagic`
    #[arg(long = "no-nmagic")]
    NoNmagic,
    /// `-fatal-warnings`
    #[arg(long = "fatal-warnings")]
    FatalWarnings,
    /// `-no-fatal-warnings`
    #[arg(long = "no-fatal-warnings")]
    NoFatalWarnings,
    /// `-w`
    #[arg(short = 'w')]
    NoWarningsShort,
    /// `-no-warnings`
    #[arg(long = "no-warnings")]
    NoWarnings,
    /// `-fork`
    #[arg(long = "fork")]
    Fork,
    /// `-no-fork`
    #[arg(long = "no-fork")]
    NoFork,
    /// `-gc-sections`
    #[arg(long = "gc-sections")]
    GcSections,
    /// `-no-gc-sections`
    #[arg(long = "no-gc-sections")]
    NoGcSections,
    /// `-print-gc-sections`
    #[arg(long = "print-gc-sections", require_equals, default_missing = "\0")]
    PrintGcSections(OsString),
    /// `-no-print-gc-sections`
    #[arg(long = "no-print-gc-sections")]
    NoPrintGcSections,
    /// `-discard-section`
    #[arg(long = "discard-section")]
    DiscardSection(OsString),
    /// `-no-discard-section`
    #[arg(long = "no-discard-section")]
    NoDiscardSection(OsString),
    /// `-icf`
    #[arg(long = "icf")]
    Icf(OsString),
    /// `-no-icf`
    #[arg(long = "no-icf")]
    NoIcf,
    /// `-ignore-data-address-equality`
    #[arg(long = "ignore-data-address-equality")]
    IgnoreDataAddressEquality,
    /// `-image-base`
    #[arg(long = "image-base")]
    ImageBase(OsString),
    /// `-physical-image-base`
    #[arg(long = "physical-image-base")]
    PhysicalImageBase(OsString),
    /// `-print-icf-sections`
    #[arg(long = "print-icf-sections", require_equals, default_missing = "\0")]
    PrintIcfSections(OsString),
    /// `-no-print-icf-sections`
    #[arg(long = "no-print-icf-sections")]
    NoPrintIcfSections,
    /// `-quick-exit`
    #[arg(long = "quick-exit")]
    QuickExit,
    /// `-no-quick-exit`
    #[arg(long = "no-quick-exit")]
    NoQuickExit,
    /// `-plugin`
    #[arg(long = "plugin")]
    Plugin(OsString),
    /// `-plugin-opt`
    #[arg(long = "plugin-opt")]
    PluginOpt(OsString),
    /// `--lto-cs-profile-generate`
    #[arg(long = "lto-cs-profile-generate", two_dashes)]
    LtoCsProfileGenerate,
    /// `--lto-debug-pass-manager`
    #[arg(long = "lto-debug-pass-manager", two_dashes)]
    LtoDebugPassManager,
    /// `-disable-verify`
    #[arg(long = "disable-verify")]
    DisableVerify,
    /// `--lto-emit-asm`
    #[arg(long = "lto-emit-asm", two_dashes)]
    LtoEmitAsm,
    /// `-no-legacy-pass-manager`
    #[arg(long = "no-legacy-pass-manager")]
    NoLegacyPassManager,
    /// `-no-lto-legacy-pass-manager`
    #[arg(long = "no-lto-legacy-pass-manager")]
    NoLtoLegacyPassManager,
    /// `--opt-remarks-with-hotness`
    #[arg(long = "opt-remarks-with-hotness", two_dashes)]
    OptRemarksWithHotness,
    /// `-lto-pseudo-probe-for-profiling`
    #[arg(long = "lto-pseudo-probe-for-profiling")]
    LtoPseudoProbeForProfiling,
    /// `-save-temps`
    #[arg(long = "save-temps")]
    SaveTemps,
    /// `-thinlto-emit-imports-files`
    #[arg(long = "thinlto-emit-imports-files")]
    ThinltoEmitImportsFiles,
    /// `-thinlto-index-only`
    #[arg(long = "thinlto-index-only", require_equals, default_missing = "\0")]
    ThinltoIndexOnly(OsString),
    /// `--lto-cs-profile-file`
    #[arg(long = "lto-cs-profile-file", two_dashes)]
    LtoCsProfileFile(OsString),
    /// `--lto-partitions`
    #[arg(long = "lto-partitions", two_dashes)]
    LtoPartitions(OsString),
    /// `--lto-obj-path`
    #[arg(long = "lto-obj-path", two_dashes)]
    LtoObjPath(OsString),
    /// `--opt-remarks-filename`
    #[arg(long = "opt-remarks-filename", two_dashes)]
    OptRemarksFilename(OsString),
    /// `--opt-remarks-format`
    #[arg(long = "opt-remarks-format", two_dashes)]
    OptRemarksFormat(OsString),
    /// `--opt-remarks-hotness-threshold`
    #[arg(long = "opt-remarks-hotness-threshold", two_dashes)]
    OptRemarksHotnessThreshold(OsString),
    /// `--opt-remarks-passes`
    #[arg(long = "opt-remarks-passes", two_dashes)]
    OptRemarksPasses(OsString),
    /// `--lto-sample-profile`
    #[arg(long = "lto-sample-profile", two_dashes)]
    LtoSampleProfile(OsString),
    /// `-thinlto-object-suffix-replace`
    #[arg(long = "thinlto-object-suffix-replace")]
    ThinltoObjectSuffixReplace(OsString),
    /// `-thinlto-prefix-replace`
    #[arg(long = "thinlto-prefix-replace")]
    ThinltoPrefixReplace(OsString),
    /// `-thinlto-cache-dir`
    #[arg(long = "thinlto-cache-dir")]
    ThinltoCacheDir(OsString),
    /// `-thinlto-cache-policy`
    #[arg(long = "thinlto-cache-policy")]
    ThinltoCachePolicy(OsString),
    /// `-thinlto-jobs`
    #[arg(long = "thinlto-jobs")]
    ThinltoJobs(OsString),
    /// `-thread-count`
    #[arg(long = "thread-count")]
    ThreadCount(OsString),
    /// `-threads`
    #[arg(long = "threads", require_equals, default_missing = "\0")]
    Threads(OsString),
    /// `-no-threads`
    #[arg(long = "no-threads")]
    NoThreads,
    /// `-discard-all`
    #[arg(long = "discard-all")]
    DiscardAll,
    /// `-x`
    #[arg(short = 'x')]
    DiscardAllShort,
    /// `-discard-locals`
    #[arg(long = "discard-locals")]
    DiscardLocals,
    /// `-X`
    #[arg(short = 'X')]
    DiscardLocalsShort,
    /// `-discard-none`
    #[arg(long = "discard-none")]
    DiscardNone,
    /// `-strip-all`
    #[arg(long = "strip-all")]
    StripAll,
    /// `-s`
    #[arg(short = 's')]
    StripAllShort,
    /// `-strip-debug`
    #[arg(long = "strip-debug")]
    StripDebug,
    /// `-S`
    #[arg(short = 'S')]
    StripDebugShort,
    /// `-warn-unresolved-symbols`
    #[arg(long = "warn-unresolved-symbols")]
    WarnUnresolvedSymbols,
    /// `-error-unresolved-symbols`
    #[arg(long = "error-unresolved-symbols")]
    ErrorUnresolvedSymbols,
    /// `-rpath`
    #[arg(long = "rpath")]
    Rpath(OsString),
    /// `-R`
    #[arg(short = 'R')]
    ShortR(OsString),
    /// `--undefined-version`
    #[arg(long = "undefined-version", two_dashes)]
    UndefinedVersion,
    /// `-no-undefined-version`
    #[arg(long = "no-undefined-version")]
    NoUndefinedVersion,
    /// `-undefined`
    #[arg(long = "undefined")]
    Undefined(OsString),
    /// `-u`
    #[arg(short = 'u')]
    UndefinedShort(OsString),
    /// `-build-id`
    #[arg(long = "build-id", require_equals, default_missing = "\0")]
    BuildId(OsString),
    /// `-no-build-id`
    #[arg(long = "no-build-id")]
    NoBuildId,
    /// `-be8`
    #[arg(long = "be8")]
    Be8,
    /// `-be32`
    #[arg(long = "be32")]
    Be32,
    /// `--format`
    #[arg(long = "format", two_dashes)]
    Format(OsString),
    /// `-b`
    #[arg(short = 'b')]
    FormatShort(OsString),
    /// `-fuse-ld`
    #[arg(long = "fuse-ld")]
    IgnoredFuseLd(OsString),
    /// `-allow-shlib-undefined`
    #[arg(long = "allow-shlib-undefined")]
    AllowShlibUndefined,
    /// `-no-allow-shlib-undefined`
    #[arg(long = "no-allow-shlib-undefined")]
    NoAllowShlibUndefined,
    /// `-O`
    #[arg(short = 'O')]
    IgnoredShortO(OsString),
    /// `-EB`
    #[arg(long = "EB")]
    IgnoredEB,
    /// `-EL`
    #[arg(long = "EL")]
    IgnoredEL,
    /// `-O0`
    #[arg(long = "O0")]
    IgnoredO0,
    /// `-O1`
    #[arg(long = "O1")]
    IgnoredO1,
    /// `-O2`
    #[arg(long = "O2")]
    IgnoredO2,
    /// `-verbose`
    #[arg(long = "verbose")]
    IgnoredVerbose,
    /// `-start-group`
    #[arg(long = "start-group")]
    IgnoredStartGroup,
    /// `-end-group`
    #[arg(long = "end-group")]
    IgnoredEndGroup,
    /// `-(`
    #[arg(short = '(')]
    IgnoredOpenParen,
    /// `-)`
    #[arg(short = ')')]
    IgnoredCloseParen,
    /// `-nostdlib`
    #[arg(long = "nostdlib")]
    IgnoredNostdlib,
    /// `-no-add-needed`
    #[arg(long = "no-add-needed")]
    IgnoredNoAddNeeded,
    /// `-no-call-graph-profile-sort`
    #[arg(long = "no-call-graph-profile-sort")]
    IgnoredNoCallGraphProfileSort,
    /// `-no-copy-dt-needed-entries`
    #[arg(long = "no-copy-dt-needed-entries")]
    IgnoredNoCopyDtNeededEntries,
    /// `-sort-section`
    #[arg(long = "sort-section")]
    IgnoredSortSection(OsString),
    /// `-sort-common`
    #[arg(long = "sort-common")]
    IgnoredSortCommon,
    /// `-dc`
    #[arg(long = "dc")]
    IgnoredDc,
    /// `-dp`
    #[arg(long = "dp")]
    IgnoredDp,
    /// `-fix-cortex-a53-835769`
    #[arg(long = "fix-cortex-a53-835769")]
    IgnoredFixCortexA53835769,
    /// `-fix-cortex-a53-843419`
    #[arg(long = "fix-cortex-a53-843419")]
    IgnoredFixCortexA53843419,
    /// `-nodefaultlibs`
    #[arg(long = "nodefaultlibs")]
    IgnoredNodefaultlibs,
    /// `-warn-constructors`
    #[arg(long = "warn-constructors")]
    IgnoredWarnConstructors,
    /// `-warn-execstack`
    #[arg(long = "warn-execstack")]
    IgnoredWarnExecstack,
    /// `-no-warn-execstack`
    #[arg(long = "no-warn-execstack")]
    IgnoredNoWarnExecstack,
    /// `-no-error-execstack`
    #[arg(long = "no-error-execstack")]
    IgnoredNoErrorExecstack,
    /// `-no-warn-rwx-segments`
    #[arg(long = "no-warn-rwx-segments")]
    IgnoredNoWarnRwxSegments,
    /// `-no-error-rwx-segments`
    #[arg(long = "no-error-rwx-segments")]
    IgnoredNoErrorRwxSegments,
    /// `-long-plt`
    #[arg(long = "long-plt")]
    IgnoredLongPlt,
    /// `-secure-plt`
    #[arg(long = "secure-plt")]
    IgnoredSecurePlt,
    /// `-rpath-link`
    #[arg(long = "rpath-link")]
    IgnoredRpathLink(OsString),
    /// `-no-keep-memory`
    #[arg(long = "no-keep-memory")]
    IgnoredNoKeepMemory,
    /// `--max-cache-size`; GNU ld reads `-max-cache-size` as `-m
    /// ax-cache-size`, so the long name needs two dashes.
    #[arg(long = "max-cache-size", two_dashes)]
    IgnoredMaxCacheSize(OsString),
    /// `--mmap-output-file`
    #[arg(long = "mmap-output-file", two_dashes)]
    IgnoredMmapOutputFile,
    /// `-no-mmap-output-file`
    #[arg(long = "no-mmap-output-file")]
    IgnoredNoMmapOutputFile,
    /// `-m`
    #[arg(short = 'm')]
    ShortMLower(OsString),
    /// `-filter`
    #[arg(long = "filter")]
    Filter(OsString),
    /// `-F`
    #[arg(short = 'F')]
    FilterShort(OsString),
    /// `-auxiliary`
    #[arg(long = "auxiliary")]
    Auxiliary(OsString),
    /// `-f`
    #[arg(short = 'f')]
    AuxiliaryShort(OsString),
    /// `-version-script`
    #[arg(long = "version-script")]
    VersionScript(OsString),
    /// `-dynamic-list`
    #[arg(long = "dynamic-list")]
    DynamicList(OsString),
    /// `-dynamic-list-data`
    #[arg(long = "dynamic-list-data")]
    DynamicListData,
    /// `--export-dynamic-symbol`
    #[arg(long = "export-dynamic-symbol", two_dashes)]
    ExportDynamicSymbol(OsString),
    /// `--export-dynamic-symbol-list`
    #[arg(long = "export-dynamic-symbol-list", two_dashes)]
    ExportDynamicSymbolList(OsString),
    /// `-entry`
    #[arg(long = "entry")]
    Entry(OsString),
    /// `-e`
    #[arg(short = 'e')]
    EntryShort(OsString),
    /// `-as-needed`
    #[arg(long = "as-needed")]
    AsNeeded,
    /// `-no-as-needed`
    #[arg(long = "no-as-needed")]
    NoAsNeeded,
    /// `-whole-archive`
    #[arg(long = "whole-archive")]
    WholeArchive,
    /// `-no-whole-archive`
    #[arg(long = "no-whole-archive")]
    NoWholeArchive,
    /// `--library`
    #[arg(long = "library", two_dashes)]
    Library(OsString),
    /// `-l`
    #[arg(short = 'l', prefix)]
    LibraryShort(OsString),
    /// `-script`
    #[arg(long = "script")]
    Script(OsString),
    /// `-T`
    #[arg(short = 'T')]
    ScriptShort(OsString),
    /// `-push-state`
    #[arg(long = "push-state")]
    PushState,
    /// `-pop-state`
    #[arg(long = "pop-state")]
    PopState,
    /// `-z KEYWORD`, `-zKEYWORD`: mapped by [`z_opt`] as it is handled;
    /// one that stays is unknown.
    #[arg(short = 'z')]
    Z(Spanned<OsString>),
    /// `-z now`.
    #[arg(skip)]
    ZNow,
    /// `-z lazy`.
    #[arg(skip)]
    ZLazy,
    /// `-z cet-report=none`.
    #[arg(skip)]
    ZCetReportNone,
    /// `-z cet-report=warning`.
    #[arg(skip)]
    ZCetReportWarning,
    /// `-z cet-report=error`.
    #[arg(skip)]
    ZCetReportError,
    /// `-z execstack`.
    #[arg(skip)]
    ZExecstack,
    /// `-z execstack-if-needed`.
    #[arg(skip)]
    ZExecstackIfNeeded,
    /// `-z max-page-size=VALUE`.
    #[arg(skip)]
    ZMaxPageSize(OsString),
    /// `-z start-stop-visibility=protected`.
    #[arg(skip)]
    ZStartStopVisibilityProtected,
    /// `-z start-stop-visibility=hidden`.
    #[arg(skip)]
    ZStartStopVisibilityHidden,
    /// `-z noexecstack`.
    #[arg(skip)]
    ZNoexecstack,
    /// `-z relro`.
    #[arg(skip)]
    ZRelro,
    /// `-z norelro`.
    #[arg(skip)]
    ZNorelro,
    /// `-z undefs`.
    #[arg(skip)]
    ZUndefs,
    /// `-z nodlopen`.
    #[arg(skip)]
    ZNodlopen,
    /// `-z nodelete`.
    #[arg(skip)]
    ZNodelete,
    /// `-z nocopyreloc`.
    #[arg(skip)]
    ZNocopyreloc,
    /// `-z nodump`.
    #[arg(skip)]
    ZNodump,
    /// `-z initfirst`.
    #[arg(skip)]
    ZInitfirst,
    /// `-z interpose`.
    #[arg(skip)]
    ZInterpose,
    /// `-z ibt`.
    #[arg(skip)]
    ZIbt,
    /// `-z ibtplt`.
    #[arg(skip)]
    ZIbtplt,
    /// `-z muldefs`.
    #[arg(skip)]
    ZMuldefs,
    /// `-z keep-text-section-prefix`.
    #[arg(skip)]
    ZKeepTextSectionPrefix,
    /// `-z nokeep-text-section-prefix`.
    #[arg(skip)]
    ZNokeepTextSectionPrefix,
    /// `-z shstk`.
    #[arg(skip)]
    ZShstk,
    /// `-z text`.
    #[arg(skip)]
    ZText,
    /// `-z notext`.
    #[arg(skip)]
    ZNotext,
    /// `-z textoff`.
    #[arg(skip)]
    ZTextoff,
    /// `-z origin`.
    #[arg(skip)]
    ZOrigin,
    /// `-z nodefaultlib`.
    #[arg(skip)]
    ZNodefaultlib,
    /// `-z separate-loadable-segments`.
    #[arg(skip)]
    ZSeparateLoadableSegments,
    /// `-z separate-code`.
    #[arg(skip)]
    ZSeparateCode,
    /// `-z noseparate-code`.
    #[arg(skip)]
    ZNoseparateCode,
    /// `-z stack-size=VALUE`.
    #[arg(skip)]
    ZStackSize(OsString),
    /// `-z dynamic-undefined-weak`.
    #[arg(skip)]
    ZDynamicUndefinedWeak,
    /// `-z nodynamic-undefined-weak`.
    #[arg(skip)]
    ZNodynamicUndefinedWeak,
    /// `-z sectionheader`.
    #[arg(skip)]
    ZSectionheader,
    /// `-z nosectionheader`.
    #[arg(skip)]
    ZNosectionheader,
    /// `-z rodynamic`.
    #[arg(skip)]
    ZRodynamic,
    /// `-z x86-64-v2`.
    #[arg(skip)]
    ZX8664V2,
    /// `-z x86-64-v3`.
    #[arg(skip)]
    ZX8664V3,
    /// `-z x86-64-v4`.
    #[arg(skip)]
    ZX8664V4,
    /// `-z rewrite-endbr`.
    #[arg(skip)]
    ZRewriteEndbr,
    /// `-z norewrite-endbr`.
    #[arg(skip)]
    ZNorewriteEndbr,
    /// A flag nothing above names, whole: `--lto-O3`, or an error.
    #[arg(unknown)]
    Unknown(OsString),
    /// Several short options in one word (`-sS`): accepted, with a warning,
    /// as GNU ld does.
    #[arg(bundle)]
    Grouped(OsString),
    /// An input file.
    #[arg(positional)]
    Input(OsString),
    /// `--help`.
    #[arg(long = "help")]
    Help,
}

/// The whole command line: every option, in order.
#[derive(Args)]
#[arg(long_only, disable_help_flag, disable_version_flag, disable_help_subcommand)]
struct Cli {
    #[arg(sequence, unknown)]
    opts: Vec<Item>,
}

/// Options that only take an attached value (`--name=value`): given bare,
/// they are unknown options, as in the built-in parser.
const EQ_ONLY: &[&str] = &[
    "--build-id",
    "--color-diagnostics",
    "--pack-dyn-relocs",
    "--package-metadata",
    "--print-gc-sections",
    "--print-icf-sections",
    "--separate-debug-file",
    "--shuffle-sections",
    "--thinlto-index-only",
    "--threads",
];

/// Parses `raw_cmdline`, which includes the program name.
pub(crate) fn parse(raw_cmdline: &[Cow<'_, OsStr>]) -> Vec<Item> {
    let words: Vec<&winnow_args::BStr> = raw_cmdline
        .iter()
        .skip(1)
        .map(|word| winnow_args::BStr::new(word.as_encoded_bytes()))
        .collect();
    match Cli::parse_words(&words) {
        Ok(cli) => cli.opts,
        Err(error) => {
            let token = error.token().unwrap_or_default();
            match error.kind() {
                ErrorKind::MissingValue if token == "-z" || EQ_ONLY.contains(&token) => {
                    fatal!("unknown command line option: {token}")
                }
                ErrorKind::MissingValue => fatal!("option {token}: argument missing"),
                ErrorKind::UnexpectedValue => fatal!(
                    "unknown command line option: {token}={}",
                    error.value().unwrap_or_default()
                ),
                _ => fatal!("{}", error.message(winnow_args::help::Style::PLAIN)),
            }
        }
    }
}

/// The option a `-z` keyword stands for, as mold reads `-z` (the exact
/// keyword, or `name=value`); `None` for an unknown one.
pub(crate) fn z_opt(word: &OsStr) -> Option<Item> {
    let word = word.to_str()?;
    Some(match word {
        "pack-relative-relocs" => Item::PackDynRelocs(OsString::from("relr")),
        "nopack-relative-relocs" => Item::PackDynRelocs(OsString::from("none")),
        "now" => Item::ZNow,
        "lazy" => Item::ZLazy,
        "cet-report=none" => Item::ZCetReportNone,
        "cet-report=warning" => Item::ZCetReportWarning,
        "cet-report=error" => Item::ZCetReportError,
        "execstack" => Item::ZExecstack,
        "execstack-if-needed" => Item::ZExecstackIfNeeded,
        "start-stop-visibility=protected" => Item::ZStartStopVisibilityProtected,
        "start-stop-visibility=hidden" => Item::ZStartStopVisibilityHidden,
        "noexecstack" => Item::ZNoexecstack,
        "relro" => Item::ZRelro,
        "norelro" => Item::ZNorelro,
        "defs" => Item::NoUndefined,
        "undefs" => Item::ZUndefs,
        "nodlopen" => Item::ZNodlopen,
        "nodelete" => Item::ZNodelete,
        "nocopyreloc" => Item::ZNocopyreloc,
        "nodump" => Item::ZNodump,
        "initfirst" => Item::ZInitfirst,
        "interpose" => Item::ZInterpose,
        "ibt" => Item::ZIbt,
        "ibtplt" => Item::ZIbtplt,
        "muldefs" => Item::ZMuldefs,
        "keep-text-section-prefix" => Item::ZKeepTextSectionPrefix,
        "nokeep-text-section-prefix" => Item::ZNokeepTextSectionPrefix,
        "shstk" => Item::ZShstk,
        "text" => Item::ZText,
        "notext" => Item::ZNotext,
        "textoff" => Item::ZTextoff,
        "origin" => Item::ZOrigin,
        "nodefaultlib" => Item::ZNodefaultlib,
        "separate-loadable-segments" => Item::ZSeparateLoadableSegments,
        "separate-code" => Item::ZSeparateCode,
        "noseparate-code" => Item::ZNoseparateCode,
        "dynamic-undefined-weak" => Item::ZDynamicUndefinedWeak,
        "nodynamic-undefined-weak" => Item::ZNodynamicUndefinedWeak,
        "sectionheader" => Item::ZSectionheader,
        "nosectionheader" => Item::ZNosectionheader,
        "rodynamic" => Item::ZRodynamic,
        "x86-64-v2" => Item::ZX8664V2,
        "x86-64-v3" => Item::ZX8664V3,
        "x86-64-v4" => Item::ZX8664V4,
        "rewrite-endbr" => Item::ZRewriteEndbr,
        "norewrite-endbr" => Item::ZNorewriteEndbr,
        "combreloc" => Item::IgnoredShortO(OsString::new()),
        "nocombreloc" => Item::IgnoredShortO(OsString::new()),
        _ => {
            if let Some(value) = word.strip_prefix("max-page-size=") {
                return Some(Item::ZMaxPageSize(OsString::from(value)));
            }
            if let Some(value) = word.strip_prefix("stack-size=") {
                return Some(Item::ZStackSize(OsString::from(value)));
            }
            if let Some(value) = word.strip_prefix("common-page-size=") {
                let _ = value;
                return Some(Item::IgnoredShortO(OsString::new()));
            }
            return None;
        }
    })
}

/// An option's value as UTF-8, as the options that read text need it.
pub(crate) fn utf8_arg<'a>(value: &'a OsStr, opt: &str) -> &'a str {
    value.to_str().unwrap_or_else(|| fatal!("option {opt}: expected a UTF-8 argument"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_items(args: &[&str]) -> Vec<Item> {
        let cmdline: Vec<_> = std::iter::once("mold")
            .chain(args.iter().copied())
            .map(|s| Cow::Borrowed(OsStr::new(s)))
            .collect();
        parse(&cmdline)
    }

    #[test]
    fn a_single_dash_long_wins_over_a_short_with_an_attached_value() {
        // As in GNU ld: a long option spelled with one dash is not a short
        // option followed by the rest of its name.
        let items =
            parse_items(&["-shared", "-entry=main", "-eh-frame-hdr", "-filter", "libf.so", "a.o"]);
        assert!(matches!(&items[0], Item::Shared));
        assert!(matches!(&items[1], Item::Entry(v) if v.as_os_str() == "main"));
        assert!(matches!(&items[2], Item::EhFrameHdr));
        assert!(matches!(&items[3], Item::Filter(v) if v.as_os_str() == "libf.so"));
        assert!(matches!(&items[4], Item::Input(v) if v.as_os_str() == "a.o"));

        // What names no long option still reads as a short one.
        let items = parse_items(&["-emain", "-Tlink.ld"]);
        assert!(matches!(&items[0], Item::EntryShort(v) if v.as_os_str() == "main"));
        assert!(matches!(&items[1], Item::ScriptShort(v) if v.as_os_str() == "link.ld"));
    }

    #[test]
    fn some_long_options_need_two_dashes_as_in_gnu_ld_and_lld() {
        // -output is -o utput, as in GNU ld.
        let items = parse_items(&["-output"]);
        assert!(matches!(&items[0], Item::OutputShort(v) if v.as_os_str() == "utput"));
        // -export-dynamic-symbol is -e xport-dynamic-symbol, so a.o is an
        // input file.
        let items = parse_items(&["-export-dynamic-symbol", "a.o"]);
        assert!(
            matches!(&items[0], Item::EntryShort(v) if v.as_os_str() == "xport-dynamic-symbol")
        );
        assert!(matches!(&items[1], Item::Input(_)));
        // -max-cache-size=1 is -m ax-cache-size=1.
        let items = parse_items(&["-max-cache-size=1"]);
        assert!(matches!(&items[0], Item::ShortMLower(v) if v.as_os_str() == "ax-cache-size=1"));
        // GNU ld reads every -lX as --library=X.
        let items = parse_items(&["-library"]);
        assert!(matches!(&items[0], Item::LibraryShort(v) if v.as_os_str() == "ibrary"));
    }
}
