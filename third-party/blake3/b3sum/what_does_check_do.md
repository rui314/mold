# How does `b3sum --check` behave exactly?<br>or: Are filepaths...text?

Most of the time, `b3sum --check` is a drop-in replacement for `md5sum --check`
and other Coreutils hashing tools. It consumes a checkfile (the output of a
regular `b3sum` command), re-hashes all the files listed there, and returns
success if all of those hashes are still correct. What makes this more
complicated than it might seem, is that representing filepaths as text means we
need to consider many possible edge cases of unrepresentable filepaths. This
document describes all of these edge cases in detail.

## The simple case

Here's the result of running `b3sum a b c/d` in a directory that contains
those three files:

```bash
$ echo hi > a
$ echo lo > b
$ mkdir c
$ echo stuff > c/d
$ b3sum a b c/d
0b8b60248fad7ac6dfac221b7e01a8b91c772421a15b387dd1fb2d6a94aee438  a
6ae4a57bbba24f79c461d30bcb4db973b9427d9207877e34d2d74528daa84115  b
2d477356c962e54784f1c5dc5297718d92087006f6ee96b08aeaf7f3cd252377  c/d
```

If we pipe that output into `b3sum --check`, it will exit with status zero
(success) and print:

```bash
$ b3sum a b c/d | b3sum --check
a: OK
b: OK
c/d: OK
```

If we delete `b` and change the contents of `c/d`, and then use the same
checkfile as above, `b3sum --check` will exit with a non-zero status (failure)
and print:

```bash
$ b3sum a b c/d > checkfile
$ rm b
$ echo more stuff >> c/d
$ b3sum --check checkfile
a: OK
b: FAILED (No such file or directory (os error 2))
c/d: FAILED
```

In these typical cases, `b3sum` and `md5sum` have identical output for success
and very similar output for failure.

## Escaping newlines and backslashes

Since the checkfile format (the regular output format of `b3sum`) is
newline-separated text, we need to worry about what happens when a filepath
contains a newline, or worse. Suppose we create a file named `x[newline]x`
(3 characters). One way to create such a file is with a Python one-liner like
this:

```python
>>> open("x\nx", "w")
```

Here's what happens when we hash that file with `b3sum`:

```bash
$ b3sum x*
\af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262  x\nx
```

Notice two things. First, `b3sum` puts a single `\` character at the front of
the line. This indicates that the filepath contains escape sequences that
`b3sum --check` will need to unescape. Then, `b3sum` replaces the newline
character in the filepath with the two-character escape sequence `\n`.
Similarly, if the filepath contained carriage returns or backslashes, `b3sum`
would escape those as `\r` and `\\` in the output. So far, all of this behavior
is still identical to `md5sum`. (Note: Coreutils [introduced `\r`
escaping](https://github.com/coreutils/coreutils/commit/ed1c58427d574fb4ff0cb8f915eb0d554000ceeb)
in v9.0, September 2021.)

## Invalid Unicode

This is where `b3sum` and `md5sum` diverge. Apart from the newline and
backslash escapes described above, `md5sum` copies all other filepath bytes
verbatim to its output. That means its output encoding is "ASCII plus whatever
bytes we got from the command line". This creates two problems:

1. Printing something that isn't UTF-8 is kind of gross.
2. Windows support.

What's the deal with Windows? To start with, there's a fundamental difference
in how Unix and Windows represent filepaths. Unix filepaths are "usually UTF-8"
and Windows filepaths are "usually UTF-16". That means that a file named `abc`
is typically represented as the bytes `[97, 98, 99]` on Unix and as the bytes
`[97, 0, 98, 0, 99, 0]` on Windows. The `md5sum` approach won't work if we plan
on creating a checkfile on Unix and checking it on Windows, or vice versa.

A more portable approach is to convert platform-specific bytes into some
consistent Unicode encoding. (In practice this is going to be UTF-8, but in
theory it could be anything.) Then when `--check` needs to open a file, we
convert the Unicode representation back into platform-specific bytes. This
makes important common cases like `abc`, and in fact even `abc[newline]def`,
work as expected. Great!

But...what did we mean above when we said *usually* UTF-8 and *usually* UTF-16?
It turns out that not every possible sequence of bytes is valid UTF-8, and not
every possible sequence of 16-bit wide chars is valid UTF-16. For example, the
byte 0xFF (255) can never appear in any UTF-8 string. If we ask Python to
decode it, it yells at us:

```python
>>> b"\xFF".decode("UTF-8")
UnicodeDecodeError: 'utf-8' codec can't decode byte 0xff in position 0: invalid start byte
```

However, tragically, we *can* create a file with that byte in its name (on
Linux at least, though not usually on macOS):

```python
>>> open(b"y\xFFy", "w")
```

So some filepaths aren't representable in Unicode at all. Our plan to "convert
platform-specific bytes into some consistent Unicode encoding" isn't going to
work for everything. What does `b3sum` do with the file above?

```bash
$ b3sum y*
af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262  y�y
```

That � in there is a "Unicode replacement character". When we run into
filepaths that we can't represent in Unicode, we replace the unrepresentable
parts with these characters. On the checking side, to avoid any possible
confusion between two different invalid filepaths, we automatically fail if we
see a replacement character. Together with a few more details covered in the
next section, this gives us an important set of properties:

1. Any file can be hashed locally.
2. Any file with a valid Unicode name not containing the � character can be
   checked.
3. Checking ambiguous or unrepresentable filepaths always fails.
4. Checkfiles are always valid UTF-8.
5. Checkfiles are portable between Unix and Windows.

## Formal Rules

1. When hashing, filepaths are represented in a platform-specific encoding,
   which can accommodate any filepath on the current platform. In Rust, this is
   `OsStr`/`OsString`.
2. In output, filepaths are first converted to UTF-8. Any non-Unicode segments
   are replaced with Unicode replacement characters (U+FFFD). In Rust, this is
   `OsStr::to_string_lossy`.
3. Then, if a filepath contains any backslashes (U+005C) or newlines (U+000A),
   these characters are escaped as `\\` and `\n` respectively.
4. Finally, any output line containing an escape sequence is prefixed with a
   single backslash.
5. When checking, each line is parsed as UTF-8, separated by a newline
   (U+000A). Invalid UTF-8 is an error.
6. Then, if a line begins with a backslash, the filepath component is
   unescaped. Any escape sequence other than `\\` or `\n` is an error. If a
   line does not begin with a backslash, unescaping is not performed, and any
   backslashes in the filepath component are interpreted literally. (`b3sum`
   output never contains unescaped backslashes, but they can occur in
   checkfiles assembled by hand.)
7. Finally, if a filepath contains a Unicode replacement character (U+FFFD) or
   a null character (U+0000), it is an error.

   **Additionally, on Windows only:**

8. In output, all backslashes (U+005C) are replaced with forward slashes
   (U+002F).
9. When checking, after unescaping, if a filepath contains a backslash, it is
   an error.
