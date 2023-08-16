use anyhow::{bail, ensure, Result};
use clap::Parser;
use std::cmp;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod unit_tests;

const NAME: &str = "b3sum";

const DERIVE_KEY_ARG: &str = "derive_key";
const KEYED_ARG: &str = "keyed";
const LENGTH_ARG: &str = "length";
const NO_NAMES_ARG: &str = "no_names";
const RAW_ARG: &str = "raw";
const CHECK_ARG: &str = "check";

#[derive(Parser)]
#[command(version, max_term_width(100))]
struct Inner {
    /// Files to hash, or checkfiles to check
    ///
    /// When no file is given, or when - is given, read standard input.
    file: Vec<PathBuf>,

    /// Use the keyed mode, reading the 32-byte key from stdin
    #[arg(long, requires("file"))]
    keyed: bool,

    /// Use the key derivation mode, with the given context string
    ///
    /// Cannot be used with --keyed.
    #[arg(long, value_name("CONTEXT"), conflicts_with(KEYED_ARG))]
    derive_key: Option<String>,

    /// The number of output bytes, before hex encoding
    #[arg(
        short,
        long,
        default_value_t = blake3::OUT_LEN as u64,
        value_name("LEN")
    )]
    length: u64,

    /// The starting output byte offset, before hex encoding
    #[arg(long, default_value_t = 0, value_name("SEEK"))]
    seek: u64,

    /// The maximum number of threads to use
    ///
    /// By default, this is the number of logical cores. If this flag is
    /// omitted, or if its value is 0, RAYON_NUM_THREADS is also respected.
    #[arg(long, value_name("NUM"))]
    num_threads: Option<usize>,

    /// Disable memory mapping
    ///
    /// Currently this also disables multithreading.
    #[arg(long)]
    no_mmap: bool,

    /// Omit filenames in the output
    #[arg(long)]
    no_names: bool,

    /// Write raw output bytes to stdout, rather than hex
    ///
    /// --no-names is implied. In this case, only a single input is allowed.
    #[arg(long)]
    raw: bool,

    /// Read BLAKE3 sums from the [FILE]s and check them
    #[arg(
        short,
        long,
        conflicts_with(DERIVE_KEY_ARG),
        conflicts_with(KEYED_ARG),
        conflicts_with(LENGTH_ARG),
        conflicts_with(RAW_ARG),
        conflicts_with(NO_NAMES_ARG)
    )]
    check: bool,

    /// Skip printing OK for each checked file
    ///
    /// Must be used with --check.
    #[arg(long, requires(CHECK_ARG))]
    quiet: bool,
}

struct Args {
    inner: Inner,
    file_args: Vec<PathBuf>,
    base_hasher: blake3::Hasher,
}

impl Args {
    fn parse() -> Result<Self> {
        // wild::args_os() is equivalent to std::env::args_os() on Unix,
        // but on Windows it adds support for globbing.
        let inner = Inner::parse_from(wild::args_os());
        let file_args = if !inner.file.is_empty() {
            inner.file.clone()
        } else {
            vec!["-".into()]
        };
        if inner.raw && file_args.len() > 1 {
            bail!("Only one filename can be provided when using --raw");
        }
        let base_hasher = if inner.keyed {
            // In keyed mode, since stdin is used for the key, we can't handle
            // `-` arguments. Input::open handles that case below.
            blake3::Hasher::new_keyed(&read_key_from_stdin()?)
        } else if let Some(ref context) = inner.derive_key {
            blake3::Hasher::new_derive_key(context)
        } else {
            blake3::Hasher::new()
        };
        Ok(Self {
            inner,
            file_args,
            base_hasher,
        })
    }

    fn num_threads(&self) -> Option<usize> {
        self.inner.num_threads
    }

    fn check(&self) -> bool {
        self.inner.check
    }

    fn raw(&self) -> bool {
        self.inner.raw
    }

    fn no_mmap(&self) -> bool {
        self.inner.no_mmap
    }

    fn no_names(&self) -> bool {
        self.inner.no_names
    }

    fn len(&self) -> u64 {
        self.inner.length
    }

    fn seek(&self) -> u64 {
        self.inner.seek
    }

    fn keyed(&self) -> bool {
        self.inner.keyed
    }

    fn quiet(&self) -> bool {
        self.inner.quiet
    }
}

enum Input {
    Mmap(io::Cursor<memmap2::Mmap>),
    File(File),
    Stdin,
}

impl Input {
    // Open an input file, using mmap if appropriate. "-" means stdin. Note
    // that this convention applies both to command line arguments, and to
    // filepaths that appear in a checkfile.
    fn open(path: &Path, args: &Args) -> Result<Self> {
        if path == Path::new("-") {
            if args.keyed() {
                bail!("Cannot open `-` in keyed mode");
            }
            return Ok(Self::Stdin);
        }
        let file = File::open(path)?;
        if !args.no_mmap() {
            if let Some(mmap) = maybe_memmap_file(&file)? {
                return Ok(Self::Mmap(io::Cursor::new(mmap)));
            }
        }
        Ok(Self::File(file))
    }

    fn hash(&mut self, args: &Args) -> Result<blake3::OutputReader> {
        let mut hasher = args.base_hasher.clone();
        match self {
            // The fast path: If we mmapped the file successfully, hash using
            // multiple threads. This doesn't work on stdin, or on some files,
            // and it can also be disabled with --no-mmap.
            Self::Mmap(cursor) => {
                hasher.update_rayon(cursor.get_ref());
            }
            // The slower paths, for stdin or files we didn't/couldn't mmap.
            // This is currently all single-threaded. Doing multi-threaded
            // hashing without memory mapping is tricky, since all your worker
            // threads have to stop every time you refill the buffer, and that
            // ends up being a lot of overhead. To solve that, we need a more
            // complicated double-buffering strategy where a background thread
            // fills one buffer while the worker threads are hashing the other
            // one. We might implement that in the future, but since this is
            // the slow path anyway, it's not high priority.
            Self::File(file) => {
                copy_wide(file, &mut hasher)?;
            }
            Self::Stdin => {
                let stdin = io::stdin();
                let lock = stdin.lock();
                copy_wide(lock, &mut hasher)?;
            }
        }
        let mut output_reader = hasher.finalize_xof();
        output_reader.set_position(args.seek());
        Ok(output_reader)
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Mmap(cursor) => cursor.read(buf),
            Self::File(file) => file.read(buf),
            Self::Stdin => io::stdin().read(buf),
        }
    }
}

// A 16 KiB buffer is enough to take advantage of all the SIMD instruction sets
// that we support, but `std::io::copy` currently uses 8 KiB. Most platforms
// can support at least 64 KiB, and there's some performance benefit to using
// bigger reads, so that's what we use here.
fn copy_wide(mut reader: impl Read, hasher: &mut blake3::Hasher) -> io::Result<u64> {
    let mut buffer = [0; 65536];
    let mut total = 0;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(total),
            Ok(n) => {
                hasher.update(&buffer[..n]);
                total += n as u64;
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

// Mmap a file, if it looks like a good idea. Return None in cases where we
// know mmap will fail, or if the file is short enough that mmapping isn't
// worth it. However, if we do try to mmap and it fails, return the error.
fn maybe_memmap_file(file: &File) -> Result<Option<memmap2::Mmap>> {
    let metadata = file.metadata()?;
    let file_size = metadata.len();
    Ok(if !metadata.is_file() {
        // Not a real file.
        None
    } else if file_size > isize::max_value() as u64 {
        // Too long to safely map.
        // https://github.com/danburkert/memmap-rs/issues/69
        None
    } else if file_size == 0 {
        // Mapping an empty file currently fails.
        // https://github.com/danburkert/memmap-rs/issues/72
        None
    } else if file_size < 16 * 1024 {
        // Mapping small files is not worth it.
        None
    } else {
        // Explicitly set the length of the memory map, so that filesystem
        // changes can't race to violate the invariants we just checked.
        let map = unsafe {
            memmap2::MmapOptions::new()
                .len(file_size as usize)
                .map(file)?
        };
        Some(map)
    })
}

fn write_hex_output(mut output: blake3::OutputReader, args: &Args) -> Result<()> {
    // Encoding multiples of the 64 bytes is most efficient.
    // TODO: This computes each output block twice when the --seek argument isn't a multiple of 64.
    // We'll refactor all of this soon anyway, once SIMD optimizations are available for the XOF.
    let mut len = args.len();
    let mut block = [0; blake3::guts::BLOCK_LEN];
    while len > 0 {
        output.fill(&mut block);
        let hex_str = hex::encode(&block[..]);
        let take_bytes = cmp::min(len, block.len() as u64);
        print!("{}", &hex_str[..2 * take_bytes as usize]);
        len -= take_bytes;
    }
    Ok(())
}

fn write_raw_output(output: blake3::OutputReader, args: &Args) -> Result<()> {
    let mut output = output.take(args.len());
    let stdout = std::io::stdout();
    let mut handler = stdout.lock();
    std::io::copy(&mut output, &mut handler)?;

    Ok(())
}

fn read_key_from_stdin() -> Result<[u8; blake3::KEY_LEN]> {
    let mut bytes = Vec::with_capacity(blake3::KEY_LEN + 1);
    let n = std::io::stdin()
        .lock()
        .take(blake3::KEY_LEN as u64 + 1)
        .read_to_end(&mut bytes)?;
    if n < blake3::KEY_LEN {
        bail!(
            "expected {} key bytes from stdin, found {}",
            blake3::KEY_LEN,
            n,
        )
    } else if n > blake3::KEY_LEN {
        bail!("read more than {} key bytes from stdin", blake3::KEY_LEN)
    } else {
        Ok(bytes[..blake3::KEY_LEN].try_into().unwrap())
    }
}

struct FilepathString {
    filepath_string: String,
    is_escaped: bool,
}

// returns (string, did_escape)
fn filepath_to_string(filepath: &Path) -> FilepathString {
    let unicode_cow = filepath.to_string_lossy();
    let mut filepath_string = unicode_cow.to_string();
    // If we're on Windows, normalize backslashes to forward slashes. This
    // avoids a lot of ugly escaping in the common case, and it makes
    // checkfiles created on Windows more likely to be portable to Unix. It
    // also allows us to set a blanket "no backslashes allowed in checkfiles on
    // Windows" rule, rather than allowing a Unix backslash to potentially get
    // interpreted as a directory separator on Windows.
    if cfg!(windows) {
        filepath_string = filepath_string.replace('\\', "/");
    }
    let mut is_escaped = false;
    if filepath_string.contains('\\') || filepath_string.contains('\n') {
        filepath_string = filepath_string.replace('\\', "\\\\").replace('\n', "\\n");
        is_escaped = true;
    }
    FilepathString {
        filepath_string,
        is_escaped,
    }
}

fn hex_half_byte(c: char) -> Result<u8> {
    // The hex characters in the hash must be lowercase for now, though we
    // could support uppercase too if we wanted to.
    if '0' <= c && c <= '9' {
        return Ok(c as u8 - '0' as u8);
    }
    if 'a' <= c && c <= 'f' {
        return Ok(c as u8 - 'a' as u8 + 10);
    }
    bail!("Invalid hex");
}

// The `check` command is a security tool. That means it's much better for a
// check to fail more often than it should (a false negative), than for a check
// to ever succeed when it shouldn't (a false positive). By forbidding certain
// characters in checked filepaths, we avoid a class of false positives where
// two different filepaths can get confused with each other.
fn check_for_invalid_characters(utf8_path: &str) -> Result<()> {
    // Null characters in paths should never happen, but they can result in a
    // path getting silently truncated on Unix.
    if utf8_path.contains('\0') {
        bail!("Null character in path");
    }
    // Because we convert invalid UTF-8 sequences in paths to the Unicode
    // replacement character, multiple different invalid paths can map to the
    // same UTF-8 string.
    if utf8_path.contains('�') {
        bail!("Unicode replacement character in path");
    }
    // We normalize all Windows backslashes to forward slashes in our output,
    // so the only natural way to get a backslash in a checkfile on Windows is
    // to construct it on Unix and copy it over. (Or of course you could just
    // doctor it by hand.) To avoid confusing this with a directory separator,
    // we forbid backslashes entirely on Windows. Note that this check comes
    // after unescaping has been done.
    if cfg!(windows) && utf8_path.contains('\\') {
        bail!("Backslash in path");
    }
    Ok(())
}

fn unescape(mut path: &str) -> Result<String> {
    let mut unescaped = String::with_capacity(2 * path.len());
    while let Some(i) = path.find('\\') {
        ensure!(i < path.len() - 1, "Invalid backslash escape");
        unescaped.push_str(&path[..i]);
        match path[i + 1..].chars().next().unwrap() {
            // Anything other than a recognized escape sequence is an error.
            'n' => unescaped.push_str("\n"),
            '\\' => unescaped.push_str("\\"),
            _ => bail!("Invalid backslash escape"),
        }
        path = &path[i + 2..];
    }
    unescaped.push_str(path);
    Ok(unescaped)
}

#[derive(Debug)]
struct ParsedCheckLine {
    file_string: String,
    is_escaped: bool,
    file_path: PathBuf,
    expected_hash: blake3::Hash,
}

fn parse_check_line(mut line: &str) -> Result<ParsedCheckLine> {
    // Trim off the trailing newline, if any.
    line = line.trim_end_matches('\n');
    // If there's a backslash at the front of the line, that means we need to
    // unescape the path below. This matches the behavior of e.g. md5sum.
    let first = if let Some(c) = line.chars().next() {
        c
    } else {
        bail!("Empty line");
    };
    let mut is_escaped = false;
    if first == '\\' {
        is_escaped = true;
        line = &line[1..];
    }
    // The front of the line must be a hash of the usual length, followed by
    // two spaces. The hex characters in the hash must be lowercase for now,
    // though we could support uppercase too if we wanted to.
    let hash_hex_len = 2 * blake3::OUT_LEN;
    let num_spaces = 2;
    let prefix_len = hash_hex_len + num_spaces;
    ensure!(line.len() > prefix_len, "Short line");
    ensure!(
        line.chars().take(prefix_len).all(|c| c.is_ascii()),
        "Non-ASCII prefix"
    );
    ensure!(&line[hash_hex_len..][..2] == "  ", "Invalid space");
    // Decode the hash hex.
    let mut hash_bytes = [0; blake3::OUT_LEN];
    let mut hex_chars = line[..hash_hex_len].chars();
    for byte in &mut hash_bytes {
        let high_char = hex_chars.next().unwrap();
        let low_char = hex_chars.next().unwrap();
        *byte = 16 * hex_half_byte(high_char)? + hex_half_byte(low_char)?;
    }
    let expected_hash: blake3::Hash = hash_bytes.into();
    let file_string = line[prefix_len..].to_string();
    let file_path_string = if is_escaped {
        // If we detected a backslash at the start of the line earlier, now we
        // need to unescape backslashes and newlines.
        unescape(&file_string)?
    } else {
        file_string.clone().into()
    };
    check_for_invalid_characters(&file_path_string)?;
    Ok(ParsedCheckLine {
        file_string,
        is_escaped,
        file_path: file_path_string.into(),
        expected_hash,
    })
}

fn hash_one_input(path: &Path, args: &Args) -> Result<()> {
    let mut input = Input::open(path, args)?;
    let output = input.hash(args)?;
    if args.raw() {
        write_raw_output(output, args)?;
        return Ok(());
    }
    if args.no_names() {
        write_hex_output(output, args)?;
        println!();
        return Ok(());
    }
    let FilepathString {
        filepath_string,
        is_escaped,
    } = filepath_to_string(path);
    if is_escaped {
        print!("\\");
    }
    write_hex_output(output, args)?;
    println!("  {}", filepath_string);
    Ok(())
}

// Returns true for success. Having a boolean return value here, instead of
// passing down the files_failed reference, makes it less likely that we might
// forget to set it in some error condition.
fn check_one_line(line: &str, args: &Args) -> bool {
    let parse_result = parse_check_line(&line);
    let ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = match parse_result {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("{}: {}", NAME, e);
            return false;
        }
    };
    let file_string = if is_escaped {
        "\\".to_string() + &file_string
    } else {
        file_string
    };
    let hash_result: Result<blake3::Hash> = Input::open(&file_path, args)
        .and_then(|mut input| input.hash(args))
        .map(|mut hash_output| {
            let mut found_hash_bytes = [0; blake3::OUT_LEN];
            hash_output.fill(&mut found_hash_bytes);
            found_hash_bytes.into()
        });
    let found_hash: blake3::Hash = match hash_result {
        Ok(hash) => hash,
        Err(e) => {
            println!("{}: FAILED ({})", file_string, e);
            return false;
        }
    };
    // This is a constant-time comparison.
    if expected_hash == found_hash {
        if !args.quiet() {
            println!("{}: OK", file_string);
        }
        true
    } else {
        println!("{}: FAILED", file_string);
        false
    }
}

fn check_one_checkfile(path: &Path, args: &Args, files_failed: &mut u64) -> Result<()> {
    let checkfile_input = Input::open(path, args)?;
    let mut bufreader = io::BufReader::new(checkfile_input);
    let mut line = String::new();
    loop {
        line.clear();
        let n = bufreader.read_line(&mut line)?;
        if n == 0 {
            return Ok(());
        }
        // check_one_line() prints errors and turns them into a success=false
        // return, so it doesn't return a Result.
        let success = check_one_line(&line, args);
        if !success {
            // We use `files_failed > 0` to indicate a mismatch, so it's important for correctness
            // that it's impossible for this counter to overflow.
            *files_failed = files_failed.saturating_add(1);
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse()?;
    let mut thread_pool_builder = rayon::ThreadPoolBuilder::new();
    if let Some(num_threads) = args.num_threads() {
        thread_pool_builder = thread_pool_builder.num_threads(num_threads);
    }
    let thread_pool = thread_pool_builder.build()?;
    thread_pool.install(|| {
        let mut files_failed = 0u64;
        // Note that file_args automatically includes `-` if nothing is given.
        for path in &args.file_args {
            if args.check() {
                check_one_checkfile(path, &args, &mut files_failed)?;
            } else {
                // Errors encountered in hashing are tolerated and printed to
                // stderr. This allows e.g. `b3sum *` to print errors for
                // non-files and keep going. However, if we encounter any
                // errors we'll still return non-zero at the end.
                let result = hash_one_input(path, &args);
                if let Err(e) = result {
                    files_failed = files_failed.saturating_add(1);
                    eprintln!("{}: {}: {}", NAME, path.to_string_lossy(), e);
                }
            }
        }
        if args.check() && files_failed > 0 {
            eprintln!(
                "{}: WARNING: {} computed checksum{} did NOT match",
                NAME,
                files_failed,
                if files_failed == 1 { "" } else { "s" },
            );
        }
        std::process::exit(if files_failed > 0 { 1 } else { 0 });
    })
}

#[cfg(test)]
mod test {
    use clap::CommandFactory;

    #[test]
    fn test_args() {
        crate::Inner::command().debug_assert();
    }
}
