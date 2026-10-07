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
    /// `-demangle`, optionally with a style; mold demangles in every style
    /// it knows, so the style is not read.
    #[arg(long = "demangle", require_equals, default_missing = "\0")]
    Demangle(OsString),
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
    /// `-t`
    #[arg(short = 't')]
    TraceShort,
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
    /// `-i`
    #[arg(short = 'i')]
    RelocatableShortI,
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
    /// `-n`
    #[arg(short = 'n')]
    NmagicShort,
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
    /// `-verbose`, optionally with a number
    #[arg(long = "verbose", require_equals, default_missing = "\0")]
    IgnoredVerbose(OsString),
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
    /// `-sort-common`, optionally with an order
    #[arg(long = "sort-common", require_equals, default_missing = "\0")]
    IgnoredSortCommon(OsString),
    /// `-dc`
    #[arg(long = "dc")]
    IgnoredDc,
    /// `-dp`
    #[arg(long = "dp")]
    IgnoredDp,
    /// `-fix-cortex-a53-835769`
    #[arg(long = "fix-cortex-a53-835769")]
    IgnoredFixCortexA53835769,
    /// `-fix-cortex-a53-843419`, optionally with a workaround
    #[arg(long = "fix-cortex-a53-843419", require_equals, default_missing = "\0")]
    IgnoredFixCortexA53843419(OsString),
    /// `-split-by-file`, optionally with a size
    #[arg(long = "split-by-file", require_equals, default_missing = "\0")]
    IgnoredSplitByFile(OsString),
    /// `-split-by-reloc`, optionally with a count
    #[arg(long = "split-by-reloc", require_equals, default_missing = "\0")]
    IgnoredSplitByReloc(OsString),
    /// `-orphan-handling` with a place; mold places no orphans
    #[arg(long = "orphan-handling")]
    IgnoredOrphanHandling(OsString),
    /// `-no-stats`
    #[arg(long = "no-stats")]
    IgnoredNoStats,
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
    // GNU ld's short options mold had no spelling for, and the long
    // names some of them stand for. -g, -d (mold defines common symbols
    // anyway), -A and -G (mold has -m), -Ur and -Qy (vendor-specific),
    // -a and -assert (HP/UX and SunOS compatibility), -Y, -c (an MRI
    // script) and -dT (a default linker script) are accepted and ignored.
    // long_only tries a single-dash word as a long option before a short
    // one takes the rest of it, so -auxiliary, -as-needed, -compress-*,
    // -cref and friends keep their meaning.
    /// `-g`
    #[arg(short = 'g')]
    IgnoredG,
    /// `-d`
    #[arg(short = 'd')]
    IgnoredD,
    /// `-A`, `--architecture`; GNU ld reads "-architecture" as "-a
    /// rchitecture", so the long name needs two dashes.
    #[arg(short = 'A', long = "architecture", two_dashes)]
    IgnoredArchitecture(OsString),
    /// `-G`, `--gpsize`
    #[arg(short = 'G', long = "gpsize")]
    IgnoredGpsize(OsString),
    /// `-Ur`
    #[arg(long = "Ur")]
    IgnoredUr,
    /// `-Qy`
    #[arg(long = "Qy")]
    IgnoredQy,
    /// `-a`, a short option whose value is attached or in the next word.
    /// long_only tries a single-dash word as a long option first, so
    /// `-auxiliary` and friends keep their meaning.
    #[arg(short = 'a')]
    IgnoredA(OsString),
    /// `-assert`
    #[arg(long = "assert")]
    IgnoredAssert(OsString),
    /// `-Y`
    #[arg(short = 'Y')]
    IgnoredY(OsString),
    /// `-c`, `--mri-script`; GNU ld reads "-mri-script" as "-m
    /// ri-script", so the long name needs two dashes.
    #[arg(short = 'c', long = "mri-script", two_dashes)]
    IgnoredMriScript(OsString),
    /// `-dT`, `--default-script`
    #[arg(long = "dT", long = "default-script")]
    IgnoredDefaultScript(OsString),
    // GNU ld's informational options, which print something, and its
    // no-op options, which do nothing on the targets mold supports. All
    // are accepted and ignored.
    /// `-print-map-discarded`
    #[arg(long = "print-map-discarded")]
    IgnoredPrintMapDiscarded,
    /// `-no-print-map-discarded`
    #[arg(long = "no-print-map-discarded")]
    IgnoredNoPrintMapDiscarded,
    /// `-print-map-locals`
    #[arg(long = "print-map-locals")]
    IgnoredPrintMapLocals,
    /// `-no-print-map-locals`
    #[arg(long = "no-print-map-locals")]
    IgnoredNoPrintMapLocals,
    /// `-strip-discarded`
    #[arg(long = "strip-discarded")]
    IgnoredStripDiscarded,
    /// `-no-strip-discarded`
    #[arg(long = "no-strip-discarded")]
    IgnoredNoStripDiscarded,
    /// `-map-whole-files`
    #[arg(long = "map-whole-files")]
    IgnoredMapWholeFiles,
    /// `-no-map-whole-files`
    #[arg(long = "no-map-whole-files")]
    IgnoredNoMapWholeFiles,
    /// `-cref`
    #[arg(long = "cref")]
    IgnoredCref,
    /// `-print-memory-usage`
    #[arg(long = "print-memory-usage")]
    IgnoredPrintMemoryUsage,
    /// `-print-sysroot`
    #[arg(long = "print-sysroot")]
    IgnoredPrintSysroot,
    /// `-print-output-format`
    #[arg(long = "print-output-format")]
    IgnoredPrintOutputFormat,
    /// `-target-help`
    #[arg(long = "target-help")]
    IgnoredTargetHelp,
    /// `-force-exe-suffix`
    #[arg(long = "force-exe-suffix")]
    IgnoredForceExeSuffix,
    /// `-traditional-format`
    #[arg(long = "traditional-format")]
    IgnoredTraditionalFormat,
    /// `-qmagic`
    #[arg(long = "qmagic")]
    IgnoredQmagic,
    /// `-reduce-memory-overheads`
    #[arg(long = "reduce-memory-overheads")]
    IgnoredReduceMemoryOverheads,
    /// `-hash-size`
    #[arg(long = "hash-size")]
    IgnoredHashSize(OsString),
    /// `-remap-inputs`
    #[arg(long = "remap-inputs")]
    IgnoredRemapInputs(OsString),
    /// `-remap-inputs-file`
    #[arg(long = "remap-inputs-file")]
    IgnoredRemapInputsFile(OsString),
    /// `-error-handling-script`
    #[arg(long = "error-handling-script")]
    IgnoredErrorHandlingScript(OsString),
    /// `-version-exports-section`
    #[arg(long = "version-exports-section")]
    IgnoredVersionExportsSection(OsString),
    /// `-accept-unknown-input-arch`
    #[arg(long = "accept-unknown-input-arch")]
    IgnoredAcceptUnknownInputArch,
    /// `-no-accept-unknown-input-arch`
    #[arg(long = "no-accept-unknown-input-arch")]
    IgnoredNoAcceptUnknownInputArch,
    /// `-no-warn-mismatch`
    #[arg(long = "no-warn-mismatch")]
    IgnoredNoWarnMismatch,
    /// `-no-warn-search-mismatch`
    #[arg(long = "no-warn-search-mismatch")]
    IgnoredNoWarnSearchMismatch,
    /// `-force-group-allocation`
    #[arg(long = "force-group-allocation")]
    IgnoredForceGroupAllocation,
    /// `-enable-non-contiguous-regions`
    #[arg(long = "enable-non-contiguous-regions")]
    IgnoredEnableNonContiguousRegions,
    /// `-enable-non-contiguous-regions-warnings`
    #[arg(long = "enable-non-contiguous-regions-warnings")]
    IgnoredEnableNonContiguousRegionsWarnings,
    /// `-disable-linker-version`
    #[arg(long = "disable-linker-version")]
    IgnoredDisableLinkerVersion,
    /// `-enable-linker-version`
    #[arg(long = "enable-linker-version")]
    IgnoredEnableLinkerVersion,
    /// `-no-enum-size-warning`
    #[arg(long = "no-enum-size-warning")]
    IgnoredNoEnumSizeWarning,
    /// `-no-wchar-size-warning`
    #[arg(long = "no-wchar-size-warning")]
    IgnoredNoWcharSizeWarning,
    /// `-default-imported-symver`
    #[arg(long = "default-imported-symver")]
    IgnoredDefaultImportedSymver,
    /// `-warn-execstack-objects`
    #[arg(long = "warn-execstack-objects")]
    IgnoredWarnExecstackObjects,
    /// `-warn-section-align`
    #[arg(long = "warn-section-align")]
    IgnoredWarnSectionAlign,
    /// `-warn-multiple-gp`
    #[arg(long = "warn-multiple-gp")]
    IgnoredWarnMultipleGp,
    /// `-warn-alternate-em`
    #[arg(long = "warn-alternate-em")]
    IgnoredWarnAlternateEm,
    /// `-error-execstack`
    #[arg(long = "error-execstack")]
    IgnoredErrorExecstack,
    /// `-warn-rwx-segments`
    #[arg(long = "warn-rwx-segments")]
    IgnoredWarnRwxSegments,
    /// `-error-rwx-segments`
    #[arg(long = "error-rwx-segments")]
    IgnoredErrorRwxSegments,
    /// `-no-define-common`
    #[arg(long = "no-define-common")]
    IgnoredNoDefineCommon,
    /// `-dynamic-list-cpp-new`
    #[arg(long = "dynamic-list-cpp-new")]
    IgnoredDynamicListCppNew,
    /// `-dynamic-list-cpp-typeinfo`
    #[arg(long = "dynamic-list-cpp-typeinfo")]
    IgnoredDynamicListCppTypeinfo,
    /// `-check-sections`
    #[arg(long = "check-sections")]
    IgnoredCheckSections,
    /// `-no-check-sections`
    #[arg(long = "no-check-sections")]
    IgnoredNoCheckSections,
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
    // GNU ld's -z keywords mold has no use for: they mark the output
    // (DF_1_GLOBAL, DF_1_UNIQUE, global auditing), or do nothing on the
    // targets mold supports.
    /// `-z global`.
    #[arg(skip)]
    ZGlobal,
    /// `-z globalaudit`.
    #[arg(skip)]
    ZGlobalaudit,
    /// `-z loadfltr`.
    #[arg(skip)]
    ZLoadfltr,
    /// `-z start-stop-gc`.
    #[arg(skip)]
    ZStartStopGc,
    /// `-z nostart-stop-gc`.
    #[arg(skip)]
    ZNoStartStopGc,
    /// `-z unique`.
    #[arg(skip)]
    ZUnique,
    /// `-z nounique`.
    #[arg(skip)]
    ZNounique,
    /// `-z unique-symbol`.
    #[arg(skip)]
    ZUniqueSymbol,
    /// `-z nounique-symbol`.
    #[arg(skip)]
    ZNouniqueSymbol,
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
        .enumerate()
        .map(|(i, word)| {
            // GNU ld reads a "-G" that names no size as "--shared" (its
            // "-lfoo" rewrite is the lexer's `prefix` rule, not a rewrite).
            if word.as_encoded_bytes() == b"-G"
                && !raw_cmdline.get(i + 2).is_some_and(|next| {
                    next.as_encoded_bytes().first().is_some_and(|b| b.is_ascii_digit())
                })
            {
                winnow_args::BStr::new(b"--shared")
            } else {
                winnow_args::BStr::new(word.as_encoded_bytes())
            }
        })
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
        "global" => Item::ZGlobal,
        "globalaudit" => Item::ZGlobalaudit,
        "loadfltr" => Item::ZLoadfltr,
        "start-stop-gc" => Item::ZStartStopGc,
        "nostart-stop-gc" => Item::ZNoStartStopGc,
        "unique" => Item::ZUnique,
        "nounique" => Item::ZNounique,
        "unique-symbol" => Item::ZUniqueSymbol,
        "nounique-symbol" => Item::ZNouniqueSymbol,
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

/// Whether a short bundle (`-sO2`) has a letter that takes a value. GNU ld
/// rejects such a word ("unable to disambiguate"), so the dispatch rejects
/// it too, instead of reading the rest of the word as that letter's value.
pub(crate) fn bundle_takes_value(word: &OsStr) -> bool {
    let Some(rest) = word.as_encoded_bytes().strip_prefix(b"-") else { return false };
    rest.iter().any(|&letter| matches!(Item::short(letter as char), Some(true)))
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

    #[test]
    fn gnu_ld_short_aliases_are_accepted() {
        // -i is -r, -n is --nmagic, -t is --trace.
        let items = parse_items(&["-i", "-n", "-t", "a.o"]);
        assert!(matches!(&items[0], Item::RelocatableShortI));
        assert!(matches!(&items[1], Item::NmagicShort));
        assert!(matches!(&items[2], Item::TraceShort));
        assert!(matches!(&items[3], Item::Input(_)));

        // The options GNU ld accepts and ignores.
        let items = parse_items(&[
            "-g",
            "-d",
            "-A",
            "x86-64",
            "--architecture=riscv64",
            "-G",
            "8",
            "--gpsize=16",
            "-Ur",
            "-Qy",
            "-a",
            "shared",
            "-ashared",
            "-assert",
            "definitions",
            "-assert=pure-text",
            "-Y",
            "/tmp",
            "-c",
            "script.mri",
            "-cscript.mri",
            "--mri-script=script.mri",
            "-dT",
            "script.ld",
            "--default-script",
            "script.ld",
            "a.o",
        ]);
        assert!(matches!(&items[0], Item::IgnoredG));
        assert!(matches!(&items[1], Item::IgnoredD));
        assert!(matches!(&items[2], Item::IgnoredArchitecture(v) if v.as_os_str() == "x86-64"));
        assert!(matches!(&items[3], Item::IgnoredArchitecture(v) if v.as_os_str() == "riscv64"));
        assert!(matches!(&items[4], Item::IgnoredGpsize(v) if v.as_os_str() == "8"));
        assert!(matches!(&items[5], Item::IgnoredGpsize(v) if v.as_os_str() == "16"));
        assert!(matches!(&items[6], Item::IgnoredUr));
        assert!(matches!(&items[7], Item::IgnoredQy));
        assert!(matches!(&items[8], Item::IgnoredA(v) if v.as_os_str() == "shared"));
        assert!(matches!(&items[9], Item::IgnoredA(v) if v.as_os_str() == "shared"));
        assert!(matches!(&items[10], Item::IgnoredAssert(v) if v.as_os_str() == "definitions"));
        assert!(matches!(&items[11], Item::IgnoredAssert(v) if v.as_os_str() == "pure-text"));
        assert!(matches!(&items[12], Item::IgnoredY(v) if v.as_os_str() == "/tmp"));
        assert!(matches!(&items[13], Item::IgnoredMriScript(v) if v.as_os_str() == "script.mri"));
        assert!(matches!(&items[14], Item::IgnoredMriScript(v) if v.as_os_str() == "script.mri"));
        assert!(matches!(&items[15], Item::IgnoredMriScript(v) if v.as_os_str() == "script.mri"));
        assert!(
            matches!(&items[16], Item::IgnoredDefaultScript(v) if v.as_os_str() == "script.ld")
        );
        assert!(
            matches!(&items[17], Item::IgnoredDefaultScript(v) if v.as_os_str() == "script.ld")
        );
        assert!(matches!(&items[18], Item::Input(_)));

        // -a and -c take their value attached as well as in a separate
        // word; a longer option that starts with the same letter keeps its
        // meaning, and a name that merely starts like -a is -a with the
        // rest of the name as its keyword.
        let items = parse_items(&["-auxiliary", "liba.so", "a.o"]);
        assert!(matches!(&items[0], Item::Auxiliary(v) if v.as_os_str() == "liba.so"));
        let items = parse_items(&["--as-needed", "a.o"]);
        assert!(matches!(&items[0], Item::AsNeeded));
        let items = parse_items(&["--compress-debug-sections=zlib", "a.o"]);
        assert!(matches!(&items[0], Item::CompressDebugSections(v) if v.as_os_str() == "zlib"));
        let items = parse_items(&["-dc", "-dp", "a.o"]);
        assert!(matches!(&items[0], Item::IgnoredDc));
        assert!(matches!(&items[1], Item::IgnoredDp));
        let items = parse_items(&["-a", "shared"]);
        assert!(matches!(&items[0], Item::IgnoredA(v) if v.as_os_str() == "shared"));
        let items = parse_items(&["-auxiliaries"]);
        assert!(matches!(&items[0], Item::IgnoredA(v) if v.as_os_str() == "uxiliaries"));
        let items = parse_items(&["-c", "script.mri"]);
        assert!(matches!(&items[0], Item::IgnoredMriScript(v) if v.as_os_str() == "script.mri"));

        // GNU ld reads "-architecture" as "-a rchitecture" and
        // "-mri-script" as "-m ri-script", so those long names need two
        // dashes.
        let items = parse_items(&["-architecture", "x86-64", "a.o"]);
        assert!(matches!(&items[0], Item::IgnoredA(v) if v.as_os_str() == "rchitecture"));
        let items = parse_items(&["-mri-script", "foo", "a.o"]);
        assert!(matches!(&items[0], Item::ShortMLower(v) if v.as_os_str() == "ri-script"));

        // A "-G" that names no size is rewritten to "--shared" before
        // parsing, and its would-be argument stays a positional input.
        let items = parse_items(&["-G", "foo", "a.o"]);
        assert!(matches!(&items[0], Item::Shared));
        assert!(matches!(&items[1], Item::Input(v) if v.as_os_str() == "foo"));
        assert!(matches!(&items[2], Item::Input(_)));
    }

    #[test]
    fn gnu_ld_optional_value_forms_are_accepted() {
        // The options whose value GNU ld makes optional: accepted bare,
        // and with the value attached by an equal sign.
        let items = parse_items(&[
            "--verbose",
            "--verbose=3",
            "--sort-common",
            "--sort-common=descending",
            "--demangle",
            "--demangle=gnu",
            "--fix-cortex-a53-843419",
            "--fix-cortex-a53-843419=adr",
            "--split-by-file",
            "--split-by-file=4096",
            "--split-by-reloc",
            "--split-by-reloc=10",
            "--orphan-handling=place",
            "--orphan-handling",
            "warn",
            "--no-stats",
            "a.o",
        ]);
        assert!(matches!(&items[0], Item::IgnoredVerbose(v) if v.as_encoded_bytes() == b"\0"));
        assert!(matches!(&items[1], Item::IgnoredVerbose(v) if v.as_os_str() == "3"));
        assert!(matches!(&items[2], Item::IgnoredSortCommon(v) if v.as_encoded_bytes() == b"\0"));
        assert!(matches!(&items[3], Item::IgnoredSortCommon(v) if v.as_os_str() == "descending"));
        assert!(matches!(&items[4], Item::Demangle(v) if v.as_encoded_bytes() == b"\0"));
        assert!(matches!(&items[5], Item::Demangle(v) if v.as_os_str() == "gnu"));
        assert!(
            matches!(&items[6], Item::IgnoredFixCortexA53843419(v) if v.as_encoded_bytes() == b"\0")
        );
        assert!(matches!(&items[7], Item::IgnoredFixCortexA53843419(v) if v.as_os_str() == "adr"));
        assert!(matches!(&items[8], Item::IgnoredSplitByFile(v) if v.as_encoded_bytes() == b"\0"));
        assert!(matches!(&items[9], Item::IgnoredSplitByFile(v) if v.as_os_str() == "4096"));
        assert!(
            matches!(&items[10], Item::IgnoredSplitByReloc(v) if v.as_encoded_bytes() == b"\0")
        );
        assert!(matches!(&items[11], Item::IgnoredSplitByReloc(v) if v.as_os_str() == "10"));
        assert!(matches!(&items[12], Item::IgnoredOrphanHandling(v) if v.as_os_str() == "place"));
        assert!(matches!(&items[13], Item::IgnoredOrphanHandling(v) if v.as_os_str() == "warn"));
        assert!(matches!(&items[14], Item::IgnoredNoStats));
        assert!(matches!(&items[15], Item::Input(v) if v.as_os_str() == "a.o"));

        // The value is attached by an equal sign only; a separate word is
        // an input file, as in GNU ld.
        let items = parse_items(&["--verbose", "3", "a.o"]);
        assert!(matches!(&items[0], Item::IgnoredVerbose(_)));
        assert!(matches!(&items[1], Item::Input(v) if v.as_os_str() == "3"));
        assert!(matches!(&items[2], Item::Input(v) if v.as_os_str() == "a.o"));

        // A name that merely starts like an option is still unknown.
        let items = parse_items(&["--sort-commonplace"]);
        assert!(matches!(&items[0], Item::Unknown(v) if v.as_os_str() == "--sort-commonplace"));
    }

    #[test]
    fn gnu_ld_no_op_options_are_accepted() {
        let items = parse_items(&[
            "--print-map-discarded",
            "--no-print-map-discarded",
            "--print-map-locals",
            "--no-print-map-locals",
            "--strip-discarded",
            "--no-strip-discarded",
            "--map-whole-files",
            "--no-map-whole-files",
            "--cref",
            "--print-memory-usage",
            "--print-sysroot",
            "--print-output-format",
            "--target-help",
            "--force-exe-suffix",
            "--traditional-format",
            "--qmagic",
            "--reduce-memory-overheads",
            "--hash-size=1024",
            "--hash-size",
            "2048",
            "--remap-inputs=a=b",
            "--remap-inputs",
            "c=d",
            "--remap-inputs-file=remap.txt",
            "--error-handling-script=err.sh",
            "--version-exports-section=VER",
            "--accept-unknown-input-arch",
            "--no-accept-unknown-input-arch",
            "--no-warn-mismatch",
            "--no-warn-search-mismatch",
            "--force-group-allocation",
            "--enable-non-contiguous-regions",
            "--enable-non-contiguous-regions-warnings",
            "--disable-linker-version",
            "--enable-linker-version",
            "--no-enum-size-warning",
            "--no-wchar-size-warning",
            "--default-imported-symver",
            "--warn-execstack-objects",
            "--warn-section-align",
            "--warn-multiple-gp",
            "--warn-alternate-em",
            "--error-execstack",
            "--warn-rwx-segments",
            "--error-rwx-segments",
            "--no-define-common",
            "--dynamic-list-cpp-new",
            "--dynamic-list-cpp-typeinfo",
            "--check-sections",
            "--no-check-sections",
            "a.o",
        ]);
        assert!(matches!(&items[0], Item::IgnoredPrintMapDiscarded));
        assert!(matches!(&items[8], Item::IgnoredCref));
        assert!(matches!(&items[17], Item::IgnoredHashSize(v) if v.as_os_str() == "1024"));
        assert!(matches!(&items[18], Item::IgnoredHashSize(v) if v.as_os_str() == "2048"));
        assert!(matches!(&items[47], Item::IgnoredNoCheckSections));
        assert!(matches!(&items[48], Item::Input(v) if v.as_os_str() == "a.o"));
        assert_eq!(items.len(), 49);

        // Single-dash spellings of long options that start with a letter
        // that is a short option: -hash-size=1024 is --hash-size=1024, not
        // -h ash-size=1024, and -qmagic is --qmagic, not -q magic.
        let items = parse_items(&["-hash-size=1024", "-qmagic", "a.o"]);
        assert!(matches!(&items[0], Item::IgnoredHashSize(v) if v.as_os_str() == "1024"));
        assert!(matches!(&items[1], Item::IgnoredQmagic));
        assert!(matches!(&items[2], Item::Input(_)));
    }

    #[test]
    fn grouped_short_flags_are_accepted_with_a_warning() {
        // -sS is Grouped("-sS") before its letters, -s and -S.
        let items = parse_items(&["-sS", "a.o"]);
        assert!(matches!(&items[0], Item::Grouped(v) if v.as_os_str() == "-sS"));
        assert!(matches!(&items[1], Item::StripAllShort));
        assert!(matches!(&items[2], Item::StripDebugShort));
        assert!(matches!(&items[3], Item::Input(_)));

        // A letter that takes a value ends the bundle, and GNU ld rejects
        // the word.
        assert!(bundle_takes_value(OsStr::new("-sO2")));
        assert!(!bundle_takes_value(OsStr::new("-sS")));
        assert!(!bundle_takes_value(OsStr::new("a.o")));
    }

    #[test]
    fn gnu_ld_z_keywords_mold_has_no_use_for_are_accepted() {
        // As the dispatch does: -z KEYWORD is the option it stands for.
        let items: Vec<Item> = parse_items(&[
            "-z",
            "global",
            "-zglobalaudit",
            "-z",
            "loadfltr",
            "-z",
            "start-stop-gc",
            "-z",
            "nostart-stop-gc",
            "-z",
            "unique",
            "-z",
            "nounique",
            "-z",
            "unique-symbol",
            "-z",
            "nounique-symbol",
            "a.o",
        ])
        .into_iter()
        .map(|item| match item {
            Item::Z(z) => z_opt(&z.value).unwrap_or(Item::Z(z)),
            item => item,
        })
        .collect();
        assert!(matches!(&items[0], Item::ZGlobal));
        assert!(matches!(&items[1], Item::ZGlobalaudit));
        assert!(matches!(&items[2], Item::ZLoadfltr));
        assert!(matches!(&items[3], Item::ZStartStopGc));
        assert!(matches!(&items[4], Item::ZNoStartStopGc));
        assert!(matches!(&items[5], Item::ZUnique));
        assert!(matches!(&items[6], Item::ZNounique));
        assert!(matches!(&items[7], Item::ZUniqueSymbol));
        assert!(matches!(&items[8], Item::ZNouniqueSymbol));
        assert!(matches!(&items[9], Item::Input(_)));
    }
}
