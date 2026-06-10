// This file implements the GNU linker script language.
//
// A linker script is, in essence, a description of how input sections
// are mapped to output sections and how the output sections are laid
// out in an output file. The language is old and quirky; for example,
// its lexical rule is context-dependent, so you cannot even tokenize
// a script without parsing it.
//
// We parse the entire language into ScriptCmd objects but currently
// evaluate only a subset of it. A command we cannot faithfully
// execute is reported as an error instead of being ignored, so that
// we never silently create an output file whose layout differs from
// what a script demands.
//
// Note that the most common real-world use of this file is not even
// SECTIONS: on Linux, /usr/lib/x86_64-linux-gnu/libc.so is not a
// shared object file but an ASCII file containing a GROUP command to
// include the real libc.so, so virtually every dynamic link against
// glibc involves linker script parsing. This file also implements
// version scripts and dynamic lists, which share the lexer.

#include "mold.h"

#include <bit>
#include <cctype>
#include <charconv>
#include <iostream>
#include <tbb/parallel_for_each.h>

namespace mold {

static std::string_view get_line(std::string_view input, const char *pos) {
  assert(input.data() <= pos);
  assert(pos < input.data() + input.size());

  i64 start = input.rfind('\n', pos - input.data());
  if (start == input.npos)
    start = 0;
  else
    start++;

  i64 end = input.find('\n', pos - input.data());
  if (end == input.npos)
    end = input.size();

  return input.substr(start, end - start);
}

template <typename E>
[[noreturn]]
static void script_error(Context<E> &ctx, MappedFile *mf,
                         std::string_view pos, std::string msg) {
  std::string_view input = mf->get_contents();
  std::string_view line = get_line(input, pos.data());

  i64 lineno = 1;
  for (i64 i = 0; input.data() + i < line.data(); i++)
    if (input[i] == '\n')
      lineno++;

  std::string label = mf->name + ":" + std::to_string(lineno) + ": ";
  i64 indent = strlen("mold: fatal: ") + label.size();
  i64 column = pos.data() - line.data();

  Fatal(ctx) << label << line << "\n"
             << std::string(indent + column, ' ') << "^ " << msg;
  unreachable();
}

// Returns the file a given token came from. It is usually `mf` but may
// be a file that the current file INCLUDE'd.
template <typename E>
MappedFile *Script<E>::file_of(std::string_view pos) {
  for (MappedFile *m : files) {
    std::string_view buf = m->get_contents();
    if (buf.data() <= pos.data() && pos.data() < buf.data() + buf.size())
      return m;
  }
  return mf;
}

template <typename E>
void Script<E>::error(std::string_view pos, std::string msg) {
  script_error(ctx, file_of(pos), pos, std::move(msg));
}

// Extracts a token from the beginning of `input` and returns it.
// Returns an empty string if there's no more token. The lexer never
// produces an empty token, so an empty return value unambiguously
// means "end of input".
template <typename E>
std::string_view Script<E>::lex_one() {
  // Skip whitespace and comments
  for (;;) {
    if (input.empty()) {
      if (include_stack.empty())
        return "";

      // Resume the file that INCLUDE'd the file we just finished
      mf = include_stack.back().first;
      input = include_stack.back().second;
      include_stack.pop_back();
      continue;
    }

    if (isspace(input[0])) {
      input = input.substr(1);
      continue;
    }

    if (input.starts_with("/*")) {
      i64 pos = input.find("*/", 2);
      if (pos == std::string_view::npos)
        error(input, "unclosed comment");
      input = input.substr(pos + 2);
      continue;
    }

    if (input[0] == '#') {
      i64 pos = input.find("\n", 1);
      if (pos == std::string_view::npos) {
        input = "";
        continue;
      }
      input = input.substr(pos + 1);
      continue;
    }
    break;
  }

  if (input[0] == '"') {
    i64 pos = input.find('"', 1);
    if (pos == std::string_view::npos)
      error(input, "unclosed string literal");
    std::string_view tok = input.substr(0, pos + 1);
    input = input.substr(pos + 1);
    return tok;
  }

  // How text is tokenized depends on the lexer mode the parser has
  // chosen. For example, `*` and `-` are part of a token in a glob
  // pattern or a filename but are operators in an expression.
  std::span<const std::string_view> ops;
  std::string_view name_chars;

  switch (lex_mode) {
  case LEX_PATH:
    name_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                 "0123456789_.$/\\~=+[]*?-!^:";
    break;
  case LEX_GLOB: {
    // Same as LEX_PATH except that `=` is an operator
    static constexpr std::string_view glob_ops[] = {
      "<<=", ">>=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "=",
    };
    ops = glob_ops;
    name_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                 "0123456789_.$/\\~+[]*?-!^:";
    break;
  }
  case LEX_EXPR: {
    static constexpr std::string_view expr_ops[] = {
      "<<", ">>", "<=", ">=", "==", "!=", "&&", "||",
    };
    ops = expr_ops;
    name_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                 "0123456789_.$";
    break;
  }
  }

  for (std::string_view op : ops) {
    if (input.starts_with(op)) {
      input = input.substr(op.size());
      return op;
    }
  }

  i64 pos = input.find_first_not_of(name_chars);

  if (pos == 0)
    pos = 1;
  else if (pos == input.npos)
    pos = input.size();

  std::string_view tok = input.substr(0, pos);
  input = input.substr(pos);
  return tok;
}

// Changes how text is tokenized. If there are tokens that have been
// peeked but not consumed yet, they were lexed in a wrong mode, so we
// un-lex and let them be re-lexed in the new mode.
template <typename E>
void Script<E>::set_lex_mode(LexMode mode) {
  if (lex_mode == mode)
    return;
  lex_mode = mode;

  if (pos < tokens.size()) {
    std::string_view buf = mf->get_contents();
    const char *start = tokens[pos].data();
    assert(buf.data() <= start && start < buf.data() + buf.size());
    input = std::string_view(start, buf.data() + buf.size() - start);
    tokens.resize(pos);
  }
}

// The following functions form a cursor into the token stream.
// peek() returns the token `n` tokens ahead without consuming it,
// or an empty string if there's no such token.
template <typename E>
std::string_view Script<E>::peek(i64 n) {
  while (tokens.size() <= pos + n) {
    std::string_view tok = lex_one();
    if (tok.empty())
      return "";
    tokens.push_back(tok);
  }
  return tokens[pos + n];
}

template <typename E>
std::string_view Script<E>::next() {
  if (peek().empty())
    Fatal(ctx) << mf->name << ": unexpected EOF";
  return tokens[pos++];
}

template <typename E>
bool Script<E>::at_eof() {
  return peek().empty();
}

template <typename E>
bool Script<E>::consume(std::string_view str) {
  if (peek() == str) {
    pos++;
    return true;
  }
  return false;
}

template <typename E>
void Script<E>::skip(std::string_view str) {
  if (at_eof())
    Fatal(ctx) << mf->name << ": expected '" << str << "', but got EOF";
  if (!consume(str))
    error(peek(), "expected '" + std::string(str) + "'");
}

// Reads a token like "global:", which may have been tokenized as a
// single token or as two tokens depending on whether the label was
// followed by a space.
template <typename E>
bool Script<E>::consume_label(std::string label) {
  if (consume(label + ":"))
    return true;

  if (peek() == label && peek(1) == ":") {
    pos += 2;
    return true;
  }
  return false;
}

// Reads ':'. Since ':' can be part of a token in the glob mode, it may
// be glued to the beginning of the token that follows it (e.g. `:text`
// as a program header reference). In that case, we split the token.
template <typename E>
bool Script<E>::consume_colon() {
  if (consume(":"))
    return true;

  if (peek().starts_with(':')) {
    tokens[pos].remove_prefix(1);
    return true;
  }
  return false;
}

static std::string_view unquote(std::string_view s) {
  if (s.size() > 0 && s[0] == '"') {
    assert(s[s.size() - 1] == '"');
    return s.substr(1, s.size() - 2);
  }
  return s;
}

// Reads a parenthesized token list and discards it
template <typename E>
void Script<E>::read_output_format() {
  skip("(");
  while (!consume(")"))
    next();
}

template <typename E>
static bool is_in_sysroot(Context<E> &ctx, std::string path) {
  std::string sysroot = ctx.arg.sysroot;
  if (sysroot.starts_with('/') && !ctx.arg.chroot.empty())
    sysroot = ctx.arg.chroot + "/" + path_clean(sysroot);

  std::string rel = std::filesystem::relative(path, sysroot).string();
  return rel != "." && !rel.starts_with("../");
}

template <typename E>
MappedFile *Script<E>::resolve_path(std::string_view tok, bool check_target) {
  std::string str(unquote(tok));

  auto open = [&](const std::string &path) -> MappedFile * {
    MappedFile *mf = open_file(ctx, path);
    if (!mf)
      return nullptr;

    if (check_target) {
      std::string_view target = get_machine_type(ctx, rctx, mf);
      if (!target.empty() && target != E::name) {
        Warn(ctx) << path << ": skipping incompatible file: " << target
                  << " (e_machine " << (int)E::e_machine << ")";
        return nullptr;
      }
    }
    return mf;
  };

  // GNU ld prepends the sysroot if a pathname starts with '/' and the
  // script being processed is in the sysroot. We do the same.
  if (str.starts_with('/') && is_in_sysroot(ctx, mf->name))
    return must_open_file(ctx, ctx.arg.sysroot + str);

  if (str.starts_with('=')) {
    std::string path;
    if (ctx.arg.sysroot.empty())
      path = str.substr(1);
    else
      path = ctx.arg.sysroot + str.substr(1);
    return must_open_file(ctx, path);
  }

  if (str.starts_with("-l"))
    return find_library(ctx, rctx, str.substr(2));

  if (!str.starts_with('/'))
    if (MappedFile *mf2 = open(path_clean(mf->name + "/../" + str)))
      return mf2;

  if (MappedFile *mf = open(str))
    return mf;

  for (std::string_view dir : ctx.arg.library_paths) {
    std::string path = std::string(dir) + "/" + str;
    if (MappedFile *mf = open(path))
      return mf;
  }

  error(tok, "library not found: " + str);
}

template <typename E>
void Script<E>::read_group() {
  skip("(");
  set_lex_mode(LEX_PATH);

  while (!consume(")")) {
    if (consume("AS_NEEDED")) {
      bool orig = rctx.as_needed;
      rctx.as_needed = true;
      read_group();
      rctx.as_needed = orig;
    } else {
      read_file(ctx, rctx, resolve_path(next(), true));
    }
  }
}

// Reads an INCLUDE command and switches the lexer's input to the
// given file. lex_one() resumes the current file when it reaches the
// end of the included file.
template <typename E>
void Script<E>::read_include() {
  if (include_stack.size() > 10)
    Fatal(ctx) << mf->name << ": INCLUDE nested too deeply";

  set_lex_mode(LEX_PATH);
  MappedFile *mf2 = resolve_path(next(), false);
  set_lex_mode(LEX_GLOB);

  assert(pos == tokens.size());
  include_stack.push_back({mf, input});
  mf = mf2;
  input = mf2->get_contents();
  files.push_back(mf2);
}

//
// Expressions
//

// Returns the precedence of a binary operator, mirroring C. Returns 0
// if the token is not a binary operator.
static i64 get_precedence(std::string_view op) {
  if (op == "*" || op == "/" || op == "%")
    return 10;
  if (op == "+" || op == "-")
    return 9;
  if (op == "<<" || op == ">>")
    return 8;
  if (op == "<" || op == ">" || op == "<=" || op == ">=")
    return 7;
  if (op == "==" || op == "!=")
    return 6;
  if (op == "&")
    return 5;
  if (op == "^")
    return 4;
  if (op == "|")
    return 3;
  if (op == "&&")
    return 2;
  if (op == "||")
    return 1;
  return 0;
}

// Parses an integer literal such as 0x1000, 0777, 100 or 4K
static std::optional<u64> parse_number(std::string_view tok) {
  u64 mul = 1;
  if (tok.ends_with('K') || tok.ends_with('k')) {
    mul = 1024;
    tok.remove_suffix(1);
  } else if (tok.ends_with('M') || tok.ends_with('m')) {
    mul = 1024 * 1024;
    tok.remove_suffix(1);
  }

  i64 base = 10;
  if (tok.starts_with("0x") || tok.starts_with("0X")) {
    tok = tok.substr(2);
    base = 16;
  } else if (tok.size() > 1 && tok[0] == '0') {
    base = 8;
  }

  u64 val;
  auto [p, ec] = std::from_chars(tok.data(), tok.data() + tok.size(), val, base);
  if (ec != std::errc() || p != tok.data() + tok.size())
    return {};
  return val * mul;
}

static bool is_script_function(std::string_view s) {
  static const std::string_view names[] = {
    "ABSOLUTE", "ADDR", "ALIGN", "ALIGNOF", "ASSERT", "BLOCK", "CONSTANT",
    "DATA_SEGMENT_ALIGN", "DATA_SEGMENT_END", "DATA_SEGMENT_RELRO_END",
    "DEFINED", "LENGTH", "LOADADDR", "LOG2CEIL", "MAX", "MIN", "NEXT",
    "ORIGIN", "SEGMENT_START", "SIZEOF",
  };
  return std::find(std::begin(names), std::end(names), s) != std::end(names);
}

template <typename E>
ScriptExpr Script<E>::read_primary() {
  std::string_view tok = next();

  if (tok == "(") {
    ScriptExpr e = read_expr();
    skip(")");
    return e;
  }

  if (tok == "-" || tok == "+" || tok == "!" || tok == "~") {
    ScriptExpr e;
    e.kind = ScriptExpr::UNARY;
    e.str = tok;
    e.args.push_back(read_primary());
    return e;
  }

  if (tok == ".") {
    ScriptExpr e;
    e.kind = ScriptExpr::DOT;
    return e;
  }

  if (isdigit(tok[0])) {
    std::optional<u64> val = parse_number(tok);
    if (!val)
      error(tok, "malformed number");

    ScriptExpr e;
    e.kind = ScriptExpr::INT;
    e.str = tok;
    e.value = *val;
    return e;
  }

  if (peek() == "(") {
    if (!is_script_function(tok))
      error(tok, "unknown linker script function");

    ScriptExpr e;
    e.kind = ScriptExpr::FUNC;
    e.str = tok;

    skip("(");
    if (!consume(")")) {
      do {
        e.args.push_back(read_expr());
      } while (consume(","));
      skip(")");
    }
    return e;
  }

  ScriptExpr e;
  e.kind = ScriptExpr::NAME;
  e.str = unquote(tok);
  return e;
}

// Reads binary expressions whose operators bind at least as tightly
// as `min_prec`, using the usual precedence climbing technique.
template <typename E>
ScriptExpr Script<E>::read_expr1(ScriptExpr lhs, i64 min_prec) {
  for (;;) {
    i64 prec = get_precedence(peek());
    if (prec < min_prec)
      return lhs;

    std::string_view op = next();
    ScriptExpr rhs = read_primary();

    while (get_precedence(peek()) > prec)
      rhs = read_expr1(std::move(rhs), prec + 1);

    ScriptExpr e;
    e.kind = ScriptExpr::BINARY;
    e.str = op;
    e.args.push_back(std::move(lhs));
    e.args.push_back(std::move(rhs));
    lhs = std::move(e);
  }
}

template <typename E>
ScriptExpr Script<E>::read_expr() {
  LexMode orig = lex_mode;
  set_lex_mode(LEX_EXPR);

  ScriptExpr e = read_expr1(read_primary(), 1);

  if (consume("?")) {
    ScriptExpr cond = std::move(e);
    e = ScriptExpr{};
    e.kind = ScriptExpr::TERNARY;
    e.args.push_back(std::move(cond));
    e.args.push_back(read_expr());
    skip(":");
    e.args.push_back(read_expr());
  }

  set_lex_mode(orig);
  return e;
}

//
// Commands
//

static bool is_assign_op(std::string_view s) {
  return s == "=" || s == "+=" || s == "-=" || s == "*=" || s == "/=" ||
         s == "%=" || s == "&=" || s == "|=" || s == "^=" || s == "<<=" ||
         s == ">>=";
}

static bool is_sort_fn(std::string_view s) {
  return s == "SORT" || s == "SORT_BY_NAME" || s == "SORT_BY_ALIGNMENT" ||
         s == "SORT_BY_INIT_PRIORITY" || s == "SORT_NONE" || s == "REVERSE";
}

static ScriptSort get_sort_kind(std::string_view s) {
  if (s == "SORT" || s == "SORT_BY_NAME")
    return ScriptSort::NAME;
  if (s == "SORT_BY_ALIGNMENT")
    return ScriptSort::ALIGNMENT;
  if (s == "SORT_BY_INIT_PRIORITY")
    return ScriptSort::INIT_PRIORITY;
  if (s == "REVERSE")
    return ScriptSort::REVERSE;
  assert(s == "SORT_NONE");
  return ScriptSort::NONE;
}

static bool is_section_type(std::string_view s) {
  return s == "NOLOAD" || s == "DSECT" || s == "COPY" || s == "INFO" ||
         s == "OVERLAY" || s == "TYPE";
}

// Reads a symbol assignment such as `foo = bar + 1;`, `. += 0x10;` or
// `PROVIDE(end = .);`.
template <typename E>
ScriptCmd Script<E>::read_assignment() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::ASSIGNMENT;
  cmd.loc = peek();

  bool paren = peek() == "PROVIDE" || peek() == "PROVIDE_HIDDEN" ||
               peek() == "HIDDEN";
  if (paren) {
    std::string_view tok = next();
    cmd.provide = (tok != "HIDDEN");
    cmd.hidden = (tok != "PROVIDE");
    skip("(");
  }

  cmd.name = unquote(next());
  cmd.op = next();
  if (!is_assign_op(cmd.op))
    error(cmd.op, "expected '='");
  cmd.value = read_expr();

  if (paren)
    skip(")");
  skip(";");
  return cmd;
}

template <typename E>
ScriptCmd Script<E>::read_assert() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::ASSERT;
  cmd.loc = peek();

  skip("ASSERT");
  skip("(");
  cmd.value = read_expr();
  skip(",");
  cmd.msg = unquote(next());
  skip(")");
  consume(";");
  return cmd;
}

// Reads an input section description such as
// `KEEP(*crtbegin*.o(EXCLUDE_FILE(*a.o) .ctors))`.
template <typename E>
ScriptCmd Script<E>::read_input_section() {
  std::string_view loc = peek();

  if (consume("KEEP")) {
    skip("(");
    ScriptCmd cmd = read_input_section();
    cmd.keep = true;
    cmd.loc = loc;
    skip(")");
    return cmd;
  }

  ScriptCmd cmd;
  cmd.kind = ScriptCmd::INPUT_SECTION;
  cmd.loc = loc;

  if (consume("INPUT_SECTION_FLAGS")) {
    skip("(");
    while (!consume(")")) {
      std::string_view tok = next();
      if (tok != "&")
        cmd.names.push_back(tok);
    }
  }

  while (consume("EXCLUDE_FILE")) {
    skip("(");
    while (!consume(")"))
      cmd.exclude_files.push_back(unquote(next()));
  }

  // A file pattern optionally wrapped in sort functions
  while (is_sort_fn(peek()) && peek(1) == "(") {
    cmd.file_sorts.push_back(get_sort_kind(next()));
    skip("(");
  }

  cmd.file_pat = unquote(next());

  for (i64 i = 0; i < cmd.file_sorts.size(); i++)
    skip(")");

  // Section patterns
  if (consume("(")) {
    while (!consume(")"))
      cmd.pats.push_back(read_pattern());
  }
  return cmd;
}

// Reads one pattern group inside the parentheses of an input section
// description. Sort functions can nest, as in
// `SORT_BY_NAME(SORT_BY_ALIGNMENT(.text.*))`.
template <typename E>
ScriptPattern Script<E>::read_pattern() {
  ScriptPattern pat;

  i64 depth = 0;
  while (is_sort_fn(peek()) && peek(1) == "(") {
    pat.sorts.push_back(get_sort_kind(next()));
    skip("(");
    depth++;
  }

  for (;;) {
    if (consume("EXCLUDE_FILE")) {
      skip("(");
      while (!consume(")"))
        pat.exclude_files.push_back(unquote(next()));
      continue;
    }

    if (peek() == ")")
      break;
    if (depth == 0 && is_sort_fn(peek()) && peek(1) == "(")
      break;
    pat.pats.push_back(unquote(next()));
  }

  for (i64 i = 0; i < depth; i++)
    skip(")");
  return pat;
}

// Reads the body of an output section description, i.e. what's
// between `{` and `}`.
template <typename E>
void Script<E>::read_output_section_body(ScriptCmd &cmd) {
  while (!consume("}")) {
    std::string_view tok = peek();

    if (consume(";"))
      continue;

    if (consume("INCLUDE")) {
      read_include();
      continue;
    }

    if (consume("CONSTRUCTORS")) {
      ScriptCmd c;
      c.kind = ScriptCmd::CONSTRUCTORS;
      c.loc = tok;
      cmd.cmds.push_back(std::move(c));
      continue;
    }

    if ((tok == "BYTE" || tok == "SHORT" || tok == "LONG" || tok == "QUAD" ||
         tok == "SQUAD") && peek(1) == "(") {
      ScriptCmd c;
      c.kind = ScriptCmd::DATA;
      c.loc = tok;
      c.op = next();
      c.data_size = (tok == "BYTE") ? 1 : (tok == "SHORT") ? 2
                  : (tok == "LONG") ? 4 : 8;
      skip("(");
      c.value = read_expr();
      skip(")");
      consume(";");
      cmd.cmds.push_back(std::move(c));
      continue;
    }

    if (tok == "FILL" && peek(1) == "(") {
      ScriptCmd c;
      c.kind = ScriptCmd::FILL;
      c.loc = tok;
      next();
      skip("(");
      c.value = read_expr();
      skip(")");
      consume(";");
      cmd.cmds.push_back(std::move(c));
      continue;
    }

    if (consume("ASCIZ")) {
      ScriptCmd c;
      c.kind = ScriptCmd::ASCIZ;
      c.loc = tok;
      c.msg = unquote(next());
      consume(";");
      cmd.cmds.push_back(std::move(c));
      continue;
    }

    if (tok == "ASSERT" && peek(1) == "(") {
      cmd.cmds.push_back(read_assert());
      continue;
    }

    if (tok == "PROVIDE" || tok == "PROVIDE_HIDDEN" || tok == "HIDDEN" ||
        is_assign_op(peek(1))) {
      cmd.cmds.push_back(read_assignment());
      continue;
    }

    cmd.cmds.push_back(read_input_section());
  }
}

// Reads `>region AT>region :phdr =fillexp` after an output section
template <typename E>
void Script<E>::read_output_section_tail(ScriptCmd &cmd) {
  if (consume(">"))
    cmd.region = unquote(next());

  if (consume("AT")) {
    skip(">");
    cmd.lma_region = unquote(next());
  }

  while (consume_colon())
    cmd.phdr_refs.push_back(unquote(next()));

  if (consume("="))
    cmd.fill = read_expr();

  consume(",");
}

// Reads an output section description such as
// `.data 0x1000 : AT(0x8000) { *(.data) } >ram AT>rom :text =0xff`.
template <typename E>
ScriptCmd Script<E>::read_output_section() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::OUTPUT_SECTION;
  cmd.loc = peek();
  cmd.name = unquote(next());

  // The colon may be glued to the section name, e.g. `.text: {`
  bool colon = false;
  if (cmd.name.size() > 1 && cmd.name.ends_with(':')) {
    cmd.name.remove_suffix(1);
    colon = true;
  }

  if (!colon) {
    // Read an optional address and an optional section type
    if (!peek().starts_with(':') &&
        !(peek() == "(" && is_section_type(peek(1))))
      cmd.addr = read_expr();

    if (peek() == "(") {
      skip("(");
      cmd.type = next();
      if (consume("="))   // (TYPE = SHT_PROGBITS)
        cmd.type = next();
      skip(")");
    }

    if (!consume_colon())
      error(peek(), "expected ':'");
  }

  for (;;) {
    if (consume("AT")) {
      skip("(");
      cmd.at = read_expr();
      skip(")");
    } else if (peek() == "ALIGN" && peek(1) == "(") {
      next();
      skip("(");
      cmd.align = read_expr();
      skip(")");
    } else if (consume("ALIGN_WITH_INPUT")) {
      cmd.align_with_input = true;
    } else if (peek() == "SUBALIGN" && peek(1) == "(") {
      next();
      skip("(");
      cmd.subalign = read_expr();
      skip(")");
    } else if (peek() == "ONLY_IF_RO" || peek() == "ONLY_IF_RW") {
      cmd.constraint = next();
    } else {
      break;
    }
  }

  skip("{");
  read_output_section_body(cmd);
  read_output_section_tail(cmd);
  return cmd;
}

// Reads an OVERLAY command, e.g.
// `OVERLAY 0x1000 : AT(0x8000) { .text0 { *(.text0) } .text1 { ... } }`
template <typename E>
ScriptCmd Script<E>::read_overlay() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::OVERLAY;
  cmd.loc = peek();
  skip("OVERLAY");

  if (!peek().starts_with(':'))
    cmd.addr = read_expr();
  if (!consume_colon())
    error(peek(), "expected ':'");

  if (consume("NOCROSSREFS"))
    cmd.names.push_back("NOCROSSREFS");

  if (consume("AT")) {
    skip("(");
    cmd.at = read_expr();
    skip(")");
  }

  skip("{");
  while (!consume("}")) {
    ScriptCmd sec;
    sec.kind = ScriptCmd::OUTPUT_SECTION;
    sec.loc = peek();
    sec.name = unquote(next());
    skip("{");
    read_output_section_body(sec);
    read_output_section_tail(sec);
    cmd.cmds.push_back(std::move(sec));
  }

  read_output_section_tail(cmd);
  return cmd;
}

template <typename E>
ScriptCmd Script<E>::read_sections() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::SECTIONS;
  cmd.loc = peek();
  skip("SECTIONS");
  skip("{");

  while (!consume("}")) {
    std::string_view tok = peek();

    if (consume(";"))
      continue;

    if (consume("INCLUDE")) {
      read_include();
      continue;
    }

    if (consume("ENTRY")) {
      ScriptCmd c;
      c.kind = ScriptCmd::ENTRY;
      c.loc = tok;
      skip("(");
      c.name = unquote(next());
      skip(")");
      consume(";");
      cmd.cmds.push_back(std::move(c));
      continue;
    }

    if (tok == "ASSERT" && peek(1) == "(") {
      cmd.cmds.push_back(read_assert());
      continue;
    }

    if (tok == "OVERLAY") {
      cmd.cmds.push_back(read_overlay());
      continue;
    }

    if (tok == "PROVIDE" || tok == "PROVIDE_HIDDEN" || tok == "HIDDEN" ||
        is_assign_op(peek(1))) {
      cmd.cmds.push_back(read_assignment());
      continue;
    }

    cmd.cmds.push_back(read_output_section());
  }
  return cmd;
}

// Reads a MEMORY command, e.g.
// `MEMORY { ram (rwx) : ORIGIN = 0x20000000, LENGTH = 256K }`
template <typename E>
ScriptCmd Script<E>::read_memory() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::MEMORY;
  cmd.loc = peek();
  skip("MEMORY");
  skip("{");

  while (!consume("}")) {
    if (consume("INCLUDE")) {
      read_include();
      continue;
    }

    ScriptRegion r;
    r.loc = peek();
    r.name = unquote(next());

    if (consume("(")) {
      r.attrs = next();
      skip(")");
    }

    if (!consume_colon())
      error(peek(), "expected ':'");

    std::string_view org = next();
    if (org != "ORIGIN" && org != "org" && org != "o")
      error(org, "expected ORIGIN");
    skip("=");
    r.origin = read_expr();

    skip(",");

    std::string_view len = next();
    if (len != "LENGTH" && len != "len" && len != "l")
      error(len, "expected LENGTH");
    skip("=");
    r.length = read_expr();

    cmd.memory.push_back(std::move(r));
  }
  return cmd;
}

template <typename E>
ScriptCmd Script<E>::read_phdrs() {
  ScriptCmd cmd;
  cmd.kind = ScriptCmd::PHDRS;
  cmd.loc = peek();
  skip("PHDRS");
  skip("{");

  while (!consume("}")) {
    ScriptPhdr p;
    p.loc = peek();
    p.name = unquote(next());
    p.type = next();

    for (;;) {
      if (consume("FILEHDR")) {
        p.filehdr = true;
      } else if (consume("PHDRS")) {
        p.phdrs = true;
      } else if (consume("AT")) {
        skip("(");
        p.at = read_expr();
        skip(")");
      } else if (consume("FLAGS")) {
        skip("(");
        p.flags = read_expr();
        skip(")");
      } else {
        break;
      }
    }

    skip(";");
    cmd.phdrs.push_back(std::move(p));
  }
  return cmd;
}

// Reads a single top-level linker script command
template <typename E>
void Script<E>::read_command() {
  std::string_view tok = peek();

  if (consume(";"))
    return;

  if (consume("INCLUDE")) {
    read_include();
    return;
  }

  if (consume("OUTPUT_FORMAT") || consume("OUTPUT_ARCH") || consume("TARGET")) {
    // These commands specify a BFD target. mold infers what to do from
    // input files themselves, so we just ignore them.
    read_output_format();
    return;
  }

  if (consume("INPUT") || consume("GROUP")) {
    read_group();
    set_lex_mode(LEX_GLOB);
    return;
  }

  if (consume("VERSION")) {
    skip("{");
    set_lex_mode(LEX_PATH);
    read_version_script();
    set_lex_mode(LEX_GLOB);
    skip("}");
    return;
  }

  if (consume("EXTERN")) {
    // EXTERN(sym ...) is equivalent to -u
    skip("(");
    while (!consume(")"))
      ctx.arg.undefined.push_back(get_symbol(ctx, unquote(next())));
    return;
  }

  if (consume("SEARCH_DIR")) {
    // SEARCH_DIR(dir) is equivalent to -L
    skip("(");
    set_lex_mode(LEX_PATH);
    ctx.arg.library_paths.push_back(std::string(unquote(next())));
    set_lex_mode(LEX_GLOB);
    skip(")");
    consume(";");
    return;
  }

  if (consume("ENTRY")) {
    ScriptCmd cmd;
    cmd.kind = ScriptCmd::ENTRY;
    cmd.loc = tok;
    skip("(");
    cmd.name = unquote(next());
    skip(")");
    consume(";");
    cmds.push_back(std::move(cmd));
    return;
  }

  if (tok == "SECTIONS") {
    cmds.push_back(read_sections());
    return;
  }

  if (tok == "MEMORY") {
    cmds.push_back(read_memory());
    return;
  }

  if (tok == "PHDRS") {
    cmds.push_back(read_phdrs());
    return;
  }

  if (tok == "ASSERT" && peek(1) == "(") {
    cmds.push_back(read_assert());
    return;
  }

  if (consume("INSERT")) {
    ScriptCmd cmd;
    cmd.kind = ScriptCmd::INSERT;
    cmd.loc = tok;
    cmd.insert_before = consume("BEFORE");
    if (!cmd.insert_before)
      skip("AFTER");
    cmd.name = unquote(next());
    consume(";");
    cmds.push_back(std::move(cmd));
    return;
  }

  if (consume("REGION_ALIAS")) {
    ScriptCmd cmd;
    cmd.kind = ScriptCmd::REGION_ALIAS;
    cmd.loc = tok;
    skip("(");
    cmd.name2 = unquote(next());
    skip(",");
    cmd.name = unquote(next());
    skip(")");
    consume(";");
    cmds.push_back(std::move(cmd));
    return;
  }

  if (consume("OUTPUT") || consume("STARTUP") || consume("NOCROSSREFS") ||
      consume("NOCROSSREFS_TO")) {
    ScriptCmd cmd;
    cmd.kind = ScriptCmd::OTHER;
    cmd.loc = tok;
    cmd.name = tok;
    skip("(");
    while (!consume(")"))
      cmd.names.push_back(unquote(next()));
    cmds.push_back(std::move(cmd));
    return;
  }

  if (consume("FORCE_COMMON_ALLOCATION") ||
      consume("INHIBIT_COMMON_ALLOCATION") ||
      consume("FORCE_GROUP_ALLOCATION")) {
    ScriptCmd cmd;
    cmd.kind = ScriptCmd::OTHER;
    cmd.loc = tok;
    cmd.name = tok;
    cmds.push_back(std::move(cmd));
    return;
  }

  if (tok == "PROVIDE" || tok == "PROVIDE_HIDDEN" || tok == "HIDDEN" ||
      is_assign_op(peek(1))) {
    cmds.push_back(read_assignment());
    return;
  }

  error(tok, "unknown linker script token");
}

// Returns true if an output section description is a plain /DISCARD/
// with input section patterns and nothing else
static bool is_plain_discard(const ScriptCmd &cmd) {
  if (cmd.name != "/DISCARD/" || cmd.addr || cmd.at || cmd.align ||
      cmd.subalign || cmd.fill || cmd.align_with_input || !cmd.type.empty() ||
      !cmd.constraint.empty() || !cmd.region.empty() ||
      !cmd.lma_region.empty() || !cmd.phdr_refs.empty())
    return false;

  for (const ScriptCmd &c : cmd.cmds)
    if (c.kind != ScriptCmd::INPUT_SECTION)
      return false;
  return true;
}

// mold cannot evaluate all linker script commands yet. We reject
// anything we cannot faithfully execute so that we never silently
// create an output file that is different from what a script demands.
template <typename E>
void Script<E>::evaluate() {
  for (ScriptCmd &cmd : cmds)
    evaluate_command(cmd);
}

template <typename E>
void Script<E>::evaluate_command(ScriptCmd &cmd) {
  if (cmd.kind == ScriptCmd::ENTRY) {
    // The -e option takes precedence over ENTRY()
    if (!ctx.arg.entry_given) {
      ctx.arg.entry = get_symbol(ctx, cmd.name);
      ctx.arg.entry->gc_root = true;
      ctx.arg.undefined.push_back(ctx.arg.entry);
    }
    return;
  }

  if (cmd.kind == ScriptCmd::ASSERT) {
    ctx.script_asserts.push_back({std::move(*cmd.value), cmd.msg,
                                  file_of(cmd.loc), cmd.loc});
    return;
  }

  if (cmd.kind == ScriptCmd::ASSIGNMENT && cmd.op == "=") {
    // `. = ASSERT(expr, "msg");` after a SECTIONS command is a common
    // idiom for a standalone assertion
    if (cmd.name == ".") {
      if (cmd.value->kind == ScriptExpr::FUNC && cmd.value->str == "ASSERT" &&
          cmd.value->args.size() == 2) {
        ctx.script_asserts.push_back({std::move(cmd.value->args[0]),
                                      cmd.value->args[1].str,
                                      file_of(cmd.loc), cmd.loc});
        return;
      }
      error(cmd.loc, "this linker script command is not supported yet");
    }

    // `foo = bar;` and `foo = 0x1000;` are equivalent to --defsym
    if (!cmd.provide && !cmd.hidden) {
      if (cmd.value->kind == ScriptExpr::NAME) {
        ctx.arg.defsyms.emplace_back(get_symbol(ctx, cmd.name),
                                     get_symbol(ctx, cmd.value->str));
        return;
      }
      if (cmd.value->kind == ScriptExpr::INT) {
        ctx.arg.defsyms.emplace_back(get_symbol(ctx, cmd.name),
                                     cmd.value->value);
        return;
      }
    }

    // Other assignments are evaluated after the layout is fixed
    register_assignment(cmd);
    return;
  }

  if (cmd.kind == ScriptCmd::SECTIONS) {
    evaluate_sections(cmd);
    return;
  }

  if (cmd.kind == ScriptCmd::PHDRS) {
    for (ScriptPhdr &p : cmd.phdrs) {
      if (p.at)
        error(p.loc, "AT is not supported yet");
      if (p.flags && p.flags->kind != ScriptExpr::INT)
        error(p.loc, "FLAGS must be an integer literal");
      ctx.script_phdrs.push_back(std::move(p));
    }
    return;
  }

  error(cmd.loc, "this linker script command is not supported yet");
}

// Creates a ScriptAssignment record for an assignment command and
// links the command to it. The symbol is defined by
// create_internal_file() or add_provided_symbols(); its value is
// computed by eval_script_commands() after the layout is fixed.
template <typename E>
void Script<E>::register_assignment(ScriptCmd &cmd) {
  cmd.aux = ctx.script_assignments.size();

  ScriptAssignment<E> a;
  a.sym = get_symbol(ctx, cmd.name);
  a.value = *cmd.value;
  a.provide = cmd.provide;
  a.hidden = cmd.hidden;
  a.mf = file_of(cmd.loc);
  a.loc = cmd.loc;
  ctx.script_assignments.push_back(std::move(a));
}

// Reads the contents of a SECTIONS command into ctx.script_sections,
// rejecting anything the layout engine cannot faithfully execute.
template <typename E>
void Script<E>::evaluate_sections(ScriptCmd &cmd) {
  for (ScriptCmd &c : cmd.cmds) {
    switch (c.kind) {
    case ScriptCmd::ENTRY:
    case ScriptCmd::ASSERT:
      evaluate_command(c);
      continue;
    case ScriptCmd::ASSIGNMENT:
      // The location counter is handled by the layout pass; other
      // symbols are registered so they are defined before symbol
      // resolution.
      if (c.name != ".") {
        if (c.op != "=")
          error(c.loc, "this linker script command is not supported yet");
        register_assignment(c);
      }
      ctx.script_sections.push_back(std::move(c));
      continue;
    case ScriptCmd::OUTPUT_SECTION:
      break;
    default:
      error(c.loc, "this linker script command is not supported yet");
    }

    if (c.name == "/DISCARD/") {
      if (!is_plain_discard(c))
        error(c.loc, "/DISCARD/ cannot have output section attributes");
      append(ctx.script_discards, std::move(c.cmds));
      continue;
    }

    if (c.subalign || c.align_with_input || !c.constraint.empty() ||
        !c.region.empty() || !c.lma_region.empty() ||
        (!c.type.empty() && c.type != "NOLOAD" && c.type != "INFO"))
      error(c.loc, "this output section attribute is not supported yet");
    if (c.fill && c.fill->kind != ScriptExpr::INT)
      error(c.loc, "a fill value must be an integer literal");

    for (ScriptCmd &b : c.cmds) {
      switch (b.kind) {
      case ScriptCmd::INPUT_SECTION:
        if (!b.file_sorts.empty() || !b.names.empty())
          error(b.loc, "this input section description is not supported yet");
        for (ScriptPattern &pat : b.pats)
          for (ScriptSort sort : pat.sorts)
            if (sort != ScriptSort::NAME && sort != ScriptSort::ALIGNMENT &&
                sort != ScriptSort::INIT_PRIORITY)
              error(b.loc, "this sort specifier is not supported yet");
        break;
      case ScriptCmd::ASSIGNMENT:
        if (b.name == ".")
          break;
        if (b.op != "=")
          error(b.loc, "this linker script command is not supported yet");
        register_assignment(b);
        break;
      case ScriptCmd::ASSERT:
        evaluate_command(b);
        break;
      case ScriptCmd::DATA:
        if (b.value->kind != ScriptExpr::INT)
          error(b.loc, "only integer literals are supported here yet");
        break;
      case ScriptCmd::CONSTRUCTORS:
        // CONSTRUCTORS is for the a.out object file format; it has
        // no effect on ELF, whose constructors are in .ctors or
        // .init_array sections.
        break;
      default:
        error(b.loc, "this linker script command is not supported yet");
      }
    }

    ctx.script_sections.push_back(std::move(c));
  }
}

template <typename E>
void Script<E>::parse_linker_script() {
  set_lex_mode(LEX_GLOB);

  while (!at_eof())
    read_command();

  if (ctx.arg.dump_script) {
    dump();
    exit(0);
  }
  evaluate();
}

//
// Expression evaluation
//

// A linker script expression evaluates to an absolute value that may
// be associated with an output section. We track the association so
// that a symbol defined as e.g. `foo = ADDR(.text) + 16;` is emitted
// as a section-relative symbol, which stays correct even if the
// output is loaded at an address other than the link-time one.
template <typename E>
struct ScriptValue {
  u64 val = 0;
  Chunk<E> *sec = nullptr;
};

template <typename E>
struct ScriptEvaluator {
  Context<E> &ctx;
  MappedFile *mf;
  std::string_view loc;

  // The value of the location counter, if it is meaningful in the
  // context this expression appears in
  std::optional<u64> dot;

  [[noreturn]] void error(std::string msg) {
    if (mf)
      script_error(ctx, mf, loc, std::move(msg));
    Fatal(ctx) << "linker script: " << msg;
  }

  // Returns the output section a given expression names. Returns a
  // null pointer for a section that is defined by a linker script but
  // didn't materialize (e.g. because nothing matched its patterns);
  // its address, size and alignment read as zero.
  Chunk<E> *get_section(const ScriptExpr &e, bool *is_live = nullptr) {
    if (e.kind != ScriptExpr::NAME)
      error("expected a section name");

    // A name the script defines means the script's section, even if a
    // linker-synthesized section happens to have the same name
    for (Chunk<E> *chunk : ctx.chunks) {
      if (chunk->script_cmd != -1 && chunk->name == e.str) {
        if (is_live)
          *is_live = true;
        return chunk;
      }
    }

    for (std::unique_ptr<OutputSection<E>> &osec : ctx.osec_pool)
      if (osec->script_cmd != -1 && osec->name == e.str)
        return osec.get();

    for (ScriptCmd &cmd : ctx.script_sections)
      if (cmd.kind == ScriptCmd::OUTPUT_SECTION && cmd.name == e.str)
        return nullptr;

    for (Chunk<E> *chunk : ctx.chunks) {
      if (chunk->name == e.str) {
        if (is_live)
          *is_live = true;
        return chunk;
      }
    }

    error("section not found: " + std::string(e.str));
  }

  // Returns the output section a given address is associated with.
  // An address on the boundary of two sections counts as part of the
  // preceding one, as in GNU ld: a symbol like `_end = .;` after the
  // last section must belong to that section, not to whatever happens
  // to start at the same address.
  Chunk<E> *section_of(u64 addr) {
    Chunk<E> *at_end = nullptr;
    Chunk<E> *at_start = nullptr;

    for (Chunk<E> *chunk : ctx.chunks) {
      if (!(chunk->shdr.sh_flags & SHF_ALLOC))
        continue;
      u64 start = chunk->shdr.sh_addr;
      u64 end = start + chunk->shdr.sh_size;
      if (start < addr && addr < end)
        return chunk;
      if (addr == end)
        at_end = chunk;
      if (addr == start && !at_start)
        at_start = chunk;
    }
    return at_end ? at_end : at_start;
  }

  u64 eval_int(const ScriptExpr &e) {
    return eval(e).val;
  }

  ScriptValue<E> eval(const ScriptExpr &e);
};

template <typename E>
ScriptValue<E> ScriptEvaluator<E>::eval(const ScriptExpr &e) {
  switch (e.kind) {
  case ScriptExpr::INT:
    return {e.value};
  case ScriptExpr::NAME: {
    if (e.str == "SIZEOF_HEADERS")
      return {ctx.ehdr->shdr.sh_size + ctx.phdr->shdr.sh_size};

    Symbol<E> *sym = get_symbol(ctx, e.str);
    if (!sym->file)
      error("undefined symbol: " + std::string(e.str));

    u64 addr = sym->get_addr(ctx);
    if (sym->is_absolute())
      return {addr};
    return {addr, section_of(addr)};
  }
  case ScriptExpr::DOT:
    if (!dot)
      error("'.' is not allowed in this context");
    return {*dot, section_of(*dot)};
  case ScriptExpr::UNARY: {
    u64 x = eval_int(e.args[0]);
    if (e.str == "-")
      return {-x};
    if (e.str == "~")
      return {~x};
    if (e.str == "!")
      return {!x};
    return {x}; // unary +
  }
  case ScriptExpr::BINARY: {
    // && and || must not evaluate the right-hand side eagerly to
    // support an idiom like `DEFINED(foo) && foo`.
    if (e.str == "&&")
      return {eval_int(e.args[0]) && eval_int(e.args[1])};
    if (e.str == "||")
      return {eval_int(e.args[0]) || eval_int(e.args[1])};

    ScriptValue<E> a = eval(e.args[0]);
    ScriptValue<E> b = eval(e.args[1]);
    u64 x = a.val;
    u64 y = b.val;

    if (e.str == "+") {
      // The sum of a section-relative value and an absolute value is
      // section-relative
      if (a.sec && b.sec)
        return {x + y};
      return {x + y, a.sec ? a.sec : b.sec};
    }
    if (e.str == "-") {
      // The difference between two section-relative values is absolute
      if (a.sec && !b.sec)
        return {x - y, a.sec};
      return {x - y};
    }

    if (e.str == "*")
      return {x * y};
    if (e.str == "/") {
      if (y == 0)
        error("division by zero");
      return {x / y};
    }
    if (e.str == "%") {
      if (y == 0)
        error("modulo by zero");
      return {x % y};
    }
    if (e.str == "<<")
      return {x << y};
    if (e.str == ">>")
      return {x >> y};
    if (e.str == "<")
      return {x < y};
    if (e.str == ">")
      return {x > y};
    if (e.str == "<=")
      return {x <= y};
    if (e.str == ">=")
      return {x >= y};
    if (e.str == "==")
      return {x == y};
    if (e.str == "!=")
      return {x != y};
    if (e.str == "&")
      return {x & y};
    if (e.str == "^")
      return {x ^ y};
    assert(e.str == "|");
    return {x | y};
  }
  case ScriptExpr::TERNARY:
    // The unselected branch must not be evaluated
    return eval(e.args[eval_int(e.args[0]) ? 1 : 2]);
  case ScriptExpr::FUNC: {
    std::string_view fn = e.str;

    auto want = [&](i64 n) {
      if (e.args.size() != n)
        error(std::string(fn) + ": wrong number of arguments");
    };

    if (fn == "ABSOLUTE") {
      want(1);
      return {eval_int(e.args[0])};
    }
    if (fn == "ADDR") {
      want(1);
      bool is_live = false;
      Chunk<E> *sec = get_section(e.args[0], &is_live);
      if (!sec)
        return {0};
      return {sec->shdr.sh_addr, is_live ? sec : nullptr};
    }
    if (fn == "SIZEOF") {
      want(1);
      Chunk<E> *sec = get_section(e.args[0]);
      return {sec ? (u64)sec->shdr.sh_size : 0};
    }
    if (fn == "ALIGNOF") {
      want(1);
      Chunk<E> *sec = get_section(e.args[0]);
      return {sec ? (u64)sec->shdr.sh_addralign : 0};
    }
    if (fn == "LOADADDR") {
      want(1);
      Chunk<E> *sec = get_section(e.args[0]);
      return {sec ? sec->lma : (u64)0};
    }
    if (fn == "DEFINED") {
      want(1);
      if (e.args[0].kind != ScriptExpr::NAME)
        error("DEFINED: expected a symbol name");
      return {get_symbol(ctx, e.args[0].str)->file != nullptr};
    }
    if (fn == "ALIGN" || fn == "BLOCK") {
      // The single-argument form aligns the location counter
      if (e.args.size() == 1) {
        if (!dot)
          error(std::string(fn) + ": '.' is not allowed in this context");
        return {align_to(*dot, eval_int(e.args[0]))};
      }
      want(2);
      return {align_to(eval_int(e.args[0]), eval_int(e.args[1]))};
    }
    if (fn == "MAX") {
      want(2);
      return {std::max(eval_int(e.args[0]), eval_int(e.args[1]))};
    }
    if (fn == "MIN") {
      want(2);
      return {std::min(eval_int(e.args[0]), eval_int(e.args[1]))};
    }
    if (fn == "LOG2CEIL") {
      want(1);
      return {(u64)std::bit_width(std::max<u64>(eval_int(e.args[0]), 1) - 1)};
    }
    if (fn == "CONSTANT") {
      want(1);
      if (e.args[0].kind == ScriptExpr::NAME && e.args[0].str == "MAXPAGESIZE")
        return {(u64)ctx.page_size};
      error("CONSTANT: unsupported constant");
    }
    if (fn == "SEGMENT_START") {
      // The second operand gives the default value, which is used
      // unless overridden with an option such as -Ttext
      want(2);
      if (e.args[0].kind == ScriptExpr::NAME) {
        std::string_view seg = e.args[0].str;
        std::string_view sec = seg == "text-segment" ? ".text"
                             : seg == "data-segment" ? ".data"
                             : seg == "bss-segment"  ? ".bss" : "";

        auto it = ctx.arg.section_start.find(sec);
        if (!sec.empty() && it != ctx.arg.section_start.end())
          return {it->second};
      }
      return {eval_int(e.args[1])};
    }
    if (fn == "ASSERT") {
      want(2);
      ScriptValue<E> v = eval(e.args[0]);
      if (!v.val)
        error(std::string(e.args[1].str));
      return v;
    }
    error(std::string(fn) + " is not supported in this context");
  }
  }
  unreachable();
}

// Computes the values of symbols defined by linker scripts and checks
// ASSERT commands. This is called at the end of fix_synthetic_symbols
// when the addresses of all sections and other symbols are final.
template <typename E>
void eval_script_commands(Context<E> &ctx) {
  for (ScriptAssignment<E> &a : ctx.script_assignments) {
    if (a.provide && !a.defined)
      continue;

    ScriptEvaluator<E> ev{ctx, a.mf, a.loc};
    if (a.has_dot)
      ev.dot = a.dot_sec ? a.dot_sec->shdr.sh_addr + a.dot_offset
                         : a.dot_offset;
    ScriptValue<E> v = ev.eval(a.value);

    if (v.sec)
      a.sym->set_output_section(v.sec);
    else
      a.sym->origin = 0;
    a.sym->value = v.val;

    if (a.hidden)
      a.sym->visibility = STV_HIDDEN;
  }

  for (ScriptAssert &a : ctx.script_asserts) {
    ScriptEvaluator<E> ev{ctx, a.mf, a.loc};
    if (!ev.eval(a.cond).val)
      script_error(ctx, a.mf, a.loc, std::string(a.msg));
  }
}

//
// Section matching
//

// Removes input sections that match a /DISCARD/ pattern in a SECTIONS
// command from the output. This pass runs before garbage collection so
// that discarded sections don't keep other sections alive.
template <typename E>
void apply_script_discards(Context<E> &ctx) {
  if (ctx.script_discards.empty())
    return;

  Timer t(ctx, "apply_script_discards");

  // A discard pattern group with its file constraints. Most groups
  // have no file constraints, in which case matching section names
  // against `sections` suffices.
  struct Candidate {
    MultiGlob sections;
    std::optional<MultiGlob> file;
    std::optional<MultiGlob> exclude_files;
  };

  std::vector<Candidate> candidates;
  MultiGlob matcher;
  bool has_file_constraint = false;

  auto compile = [&](MultiGlob &glob, std::span<const std::string_view> pats) {
    for (std::string_view pat : pats)
      if (!glob.add(pat, 1))
        Fatal(ctx) << "/DISCARD/: invalid glob pattern: " << pat;
    glob.compile();
  };

  for (ScriptCmd &spec : ctx.script_discards) {
    // A bare file pattern (e.g. `foo.o` without a section list)
    // discards all sections of matching files
    if (spec.pats.empty())
      spec.pats.push_back({.pats = {"*"}});

    for (const ScriptPattern &pat : spec.pats) {
      Candidate &c = candidates.emplace_back();
      compile(c.sections, pat.pats);

      if (spec.file_pat != "*") {
        c.file.emplace();
        compile(*c.file, std::array{spec.file_pat});
      }

      std::vector<std::string_view> excludes;
      append(excludes, spec.exclude_files);
      append(excludes, pat.exclude_files);
      if (!excludes.empty()) {
        c.exclude_files.emplace();
        compile(*c.exclude_files, excludes);
      }

      has_file_constraint |= c.file.has_value() || c.exclude_files.has_value();

      for (std::string_view p : pat.pats)
        if (!matcher.add(p, 1))
          Fatal(ctx) << "/DISCARD/: invalid glob pattern: " << p;
    }
  }
  matcher.compile();

  // We can't discard merged sections yet because their contents have
  // already been split into section pieces.
  for (std::unique_ptr<MergedSection<E>> &sec : ctx.merged_sections)
    if (matcher.find(sec->name) != -1)
      Fatal(ctx) << "/DISCARD/: discarding a mergeable section is not "
                 << "supported yet: " << sec->name;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (!isec || !isec->is_alive || matcher.find(isec->name) == -1)
        continue;

      if (!has_file_constraint) {
        isec->kill();
        continue;
      }

      auto matches = [&](Candidate &c) {
        return c.sections.find(isec->name) != -1 &&
               (!c.file || c.file->find(file->filename) != -1) &&
               (!c.exclude_files ||
                c.exclude_files->find(file->filename) == -1);
      };

      for (Candidate &c : candidates) {
        if (matches(c)) {
          isec->kill();
          break;
        }
      }
    }
  });
}

// Matches all input sections against the section patterns in SECTIONS
// commands. The result is stored in each file's `script_match` vector
// and drives output section formation, member ordering, KEEP and
// garbage collection.
template <typename E>
void match_script_sections(Context<E> &ctx) {
  if (ctx.script_sections.empty())
    return;

  Timer t(ctx, "match_script_sections");

  struct Candidate {
    MultiGlob sections;
    std::optional<MultiGlob> file;
    std::optional<MultiGlob> exclude_files;
  };

  std::vector<Candidate> candidates;
  MultiGlob prefilter;
  bool has_file_constraint = false;

  auto compile = [&](MultiGlob &glob, std::span<const std::string_view> pats,
                     i64 val) {
    for (std::string_view pat : pats)
      if (!glob.add(pat, val))
        Fatal(ctx) << "linker script: invalid glob pattern: " << pat;
    glob.compile();
  };

  // Flatten all patterns into candidates. A pattern's rank is simply
  // its position in the script, which gives us both the first-match-
  // wins rule and the member ordering for free.
  for (i64 i = 0; i < ctx.script_sections.size(); i++) {
    ScriptCmd &osec = ctx.script_sections[i];
    if (osec.kind != ScriptCmd::OUTPUT_SECTION)
      continue;

    for (ScriptCmd &spec : osec.cmds) {
      if (spec.kind != ScriptCmd::INPUT_SECTION)
        continue;

      if (spec.pats.empty())
        spec.pats.push_back({.pats = {"*"}});
      spec.aux = candidates.size();

      for (ScriptPattern &pat : spec.pats) {
        i64 rank = candidates.size();
        Candidate &c = candidates.emplace_back();
        compile(c.sections, pat.pats, 0);

        if (spec.file_pat != "*") {
          c.file.emplace();
          compile(*c.file, std::array{spec.file_pat}, 0);
        }

        std::vector<std::string_view> excludes;
        append(excludes, spec.exclude_files);
        append(excludes, pat.exclude_files);
        if (!excludes.empty()) {
          c.exclude_files.emplace();
          compile(*c.exclude_files, excludes, 0);
        }

        has_file_constraint |=
          c.file.has_value() || c.exclude_files.has_value();

        for (std::string_view p : pat.pats)
          if (!prefilter.add(p, rank))
            Fatal(ctx) << "linker script: invalid glob pattern: " << p;

        ctx.script_ranks.push_back({(i32)i, spec.keep, pat.sorts});
      }
    }
  }
  prefilter.compile();

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->script_match.resize(file->sections.size(), -1);

    for (i64 j = 0; j < file->sections.size(); j++) {
      InputSection<E> *isec = file->sections[j].get();
      if (!isec || !isec->is_alive)
        continue;

      // The prefilter returns the first candidate whose section
      // pattern matches. That's the answer unless file constraints
      // are in play.
      i64 rank = prefilter.find(isec->name);
      if (rank == -1)
        continue;

      if (has_file_constraint) {
        auto matches = [&](Candidate &c) {
          return c.sections.find(isec->name) != -1 &&
                 (!c.file || c.file->find(file->filename) != -1) &&
                 (!c.exclude_files ||
                  c.exclude_files->find(file->filename) == -1);
        };

        while (rank < candidates.size() && !matches(candidates[rank]))
          rank++;
        if (rank == candidates.size())
          continue;
      }

      file->script_match[j] = rank;
    }
  });
}

// Parses the priority number of a section name such as
// `.init_array.10000`. Sections without a number sort last, as GNU ld
// does.
static i64 get_init_priority(std::string_view name) {
  size_t pos = name.find_last_not_of("0123456789");
  if (pos == name.npos || pos + 1 == name.size())
    return 1 << 30;

  i64 val = 0;
  std::from_chars(name.data() + pos + 1, name.data() + name.size(), val);
  return val;
}

// Applies an output section description to the section that was
// created for it: orders the members by match rank and SORT
// specifiers and sets the attributes the script requests.
template <typename E>
void script_finalize_section(Context<E> &ctx, OutputSection<E> &osec) {
  ScriptCmd &cmd = ctx.script_sections[osec.script_cmd];

  auto get_rank = [&](InputSection<E> *isec) -> i32 {
    return isec->file.script_match[isec->shndx];
  };

  ranges::stable_sort(osec.members,
                      [&](InputSection<E> *a, InputSection<E> *b) {
    i32 ra = get_rank(a);
    i32 rb = get_rank(b);
    if (ra != rb)
      return ra < rb;

    for (ScriptSort sort : ctx.script_ranks[ra].sorts) {
      switch (sort) {
      case ScriptSort::NAME:
        if (a->name != b->name)
          return a->name < b->name;
        break;
      case ScriptSort::ALIGNMENT:
        if (a->p2align != b->p2align)
          return a->p2align > b->p2align;
        break;
      case ScriptSort::INIT_PRIORITY: {
        i64 pa = get_init_priority(a->name);
        i64 pb = get_init_priority(b->name);
        if (pa != pb)
          return pa < pb;
        break;
      }
      default:
        break;
      }
    }
    return false;
  });

  // The section type is that of the members if they agree, otherwise
  // PROGBITS. A NOLOAD section is NOBITS regardless.
  u32 type = SHT_PROGBITS;
  if (!osec.members.empty()) {
    type = osec.members[0]->shdr().sh_type;
    for (InputSection<E> *isec : osec.members)
      if (isec->shdr().sh_type != type)
        type = SHT_PROGBITS;
  }
  if (cmd.type == "NOLOAD")
    type = SHT_NOBITS;
  osec.shdr.sh_type = type;

  if (cmd.type == "INFO")
    osec.shdr.sh_flags &= ~(u64)SHF_ALLOC;

  if (cmd.fill)
    osec.fill = cmd.fill->value;
}

// Evaluates a location counter expression in an output section body.
// `dot` is the current absolute location. Returns nullopt for
// expressions that need more than the location counter, constants and
// previously defined body symbols.
static std::optional<u64>
eval_dot_expr(const ScriptExpr &e, u64 dot,
              std::unordered_map<std::string_view, u64> &local) {
  auto eval = [&](const ScriptExpr &e) {
    return eval_dot_expr(e, dot, local);
  };

  switch (e.kind) {
  case ScriptExpr::INT:
    return {e.value};
  case ScriptExpr::DOT:
    return {dot};
  case ScriptExpr::NAME:
    if (auto it = local.find(e.str); it != local.end())
      return {it->second};
    return {};
  case ScriptExpr::UNARY: {
    std::optional<u64> x = eval(e.args[0]);
    if (!x)
      return {};
    if (e.str == "-")
      return {-*x};
    if (e.str == "~")
      return {~*x};
    if (e.str == "!")
      return {(u64)!*x};
    return x;
  }
  case ScriptExpr::BINARY: {
    std::optional<u64> a = eval(e.args[0]);
    std::optional<u64> b = eval(e.args[1]);
    if (!a || !b)
      return {};
    if (e.str == "+")
      return {*a + *b};
    if (e.str == "-")
      return {*a - *b};
    if (e.str == "*")
      return {*a * *b};
    if (e.str == "/" && *b)
      return {*a / *b};
    if (e.str == "<<")
      return {*a << *b};
    if (e.str == ">>")
      return {*a >> *b};
    if (e.str == "&")
      return {*a & *b};
    if (e.str == "|")
      return {*a | *b};
    return {};
  }
  case ScriptExpr::FUNC: {
    if (e.str == "ALIGN" && e.args.size() == 1) {
      if (std::optional<u64> v = eval(e.args[0]))
        return {align_to(dot, *v)};
      return {};
    }

    if (e.args.size() != 2)
      return {};
    std::optional<u64> a = eval(e.args[0]);
    std::optional<u64> b = eval(e.args[1]);
    if (!a || !b)
      return {};
    if (e.str == "ALIGN")
      return {align_to(*a, *b)};
    if (e.str == "MAX")
      return {std::max(*a, *b)};
    if (e.str == "MIN")
      return {std::min(*a, *b)};
    return {};
  }
  default:
    return {};
  }
}

// Lets a linker script place linker-synthesized sections such as
// .dynamic, .hash or .eh_frame: a synthesized chunk is assigned to
// the output section description whose patterns match its name, as if
// it were an input section. A description that already got a section
// from input section matching is not eligible.
template <typename E>
void match_synthetic_sections(Context<E> &ctx) {
  std::vector<bool> taken(ctx.script_sections.size());
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->script_cmd != -1)
      taken[chunk->script_cmd] = true;

  MultiGlob matcher;
  for (i64 i = 0; i < ctx.script_sections.size(); i++) {
    ScriptCmd &osec = ctx.script_sections[i];
    if (osec.kind != ScriptCmd::OUTPUT_SECTION)
      continue;
    for (ScriptCmd &spec : osec.cmds)
      if (spec.kind == ScriptCmd::INPUT_SECTION)
        for (ScriptPattern &pat : spec.pats)
          for (std::string_view p : pat.pats)
            matcher.add(p, i);
  }
  if (matcher.empty())
    return;
  matcher.compile();

  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk->script_cmd != -1 || chunk->to_osec() || chunk->is_header())
      continue;

    // We create .got and .got.plt unconditionally for conventions
    // such as _GLOBAL_OFFSET_TABLE_ even if no GOT entry is needed.
    // GNU ld creates them on demand, so a placeholder-only table must
    // not be placed (and thereby sized) by a description like the
    // Linux kernel's `.got : { *(.got) }`, whose
    // ASSERT(SIZEOF(.got) == 0) verifies that no GOT is needed.
    i64 got_min = (is_s390x<E> ? 3 : 1) * sizeof(Word<E>);
    if ((chunk == ctx.got && chunk->shdr.sh_size <= got_min) ||
        (chunk == ctx.gotplt &&
         chunk->shdr.sh_size <= GotPltSection<E>::HDR_SIZE))
      continue;

    i64 cmd = matcher.find(chunk->name);
    if (cmd != -1 && !taken[cmd]) {
      chunk->script_cmd = cmd;
      taken[cmd] = true;
    }
  }
}

// Computes member offsets and the size of an output section described
// by a linker script, executing the location counter arithmetic in
// its body. `base` is the section's address; the location counter is
// absolute, so the result depends on it. This function is first run
// with a provisional base of zero to get an estimated size, then re-
// run by the layout pass with the real address.
template <typename E>
void script_compute_section_size(Context<E> &ctx, OutputSection<E> &osec,
                                 u64 base) {
  ScriptCmd &cmd = ctx.script_sections[osec.script_cmd];

  // Values of symbols defined earlier in this body, for expressions
  // like `. = __start_init_stack + 0x4000;`
  std::unordered_map<std::string_view, u64> local;

  u64 dot = base;
  u64 align = 1;
  i64 m = 0;

  osec.script_data.clear();

  for (ScriptCmd &b : cmd.cmds) {
    switch (b.kind) {
    case ScriptCmd::DATA:
      osec.script_data.push_back({dot - base, b.data_size, b.value->value});
      dot += b.data_size;
      break;
    case ScriptCmd::INPUT_SECTION: {
      // Place the members this description matched. Members are
      // sorted by rank, so they are consecutive.
      i64 hi = b.aux + b.pats.size();
      for (; m < osec.members.size(); m++) {
        InputSection<E> *isec = osec.members[m];
        if (isec->file.script_match[isec->shndx] >= hi)
          break;
        dot = align_to(dot, (u64)1 << isec->p2align);
        align = std::max<u64>(align, (u64)1 << isec->p2align);
        isec->offset = dot - base;
        dot += isec->sh_size;
      }
      break;
    }
    case ScriptCmd::ASSIGNMENT: {
      if (b.name == ".") {
        std::optional<u64> v = eval_dot_expr(*b.value, dot, local);
        if (!v)
          Fatal(ctx) << "linker script: unsupported expression for `.` in "
                     << "section " << osec.name;
        u64 val = (b.op == "+=") ? dot + *v : *v;
        if (val < dot)
          Fatal(ctx) << "linker script: `.` cannot move backward in section "
                     << osec.name;
        dot = val;
        break;
      }

      // Bind the location counter for post-layout evaluation
      ScriptAssignment<E> &a = ctx.script_assignments[b.aux];
      a.has_dot = true;
      a.dot_sec = &osec;
      a.dot_offset = dot - base;

      if (std::optional<u64> v = eval_dot_expr(*b.value, dot, local))
        local[b.name] = *v;
      break;
    }
    default:
      break;
    }
  }

  osec.shdr.sh_size = dot - base;
  osec.shdr.sh_addralign = align;
}

// Assigns virtual and load addresses to output sections as described
// by SECTIONS. This is one of the three layout modes and, like the
// others, may be executed multiple times until the layout converges.
template <typename E>
void set_virtual_addresses_by_script(Context<E> &ctx) {
  // Map each output section command to its chunk. A chunk that was
  // dropped for being empty is mapped too but marked dead; symbols
  // bound to it still need an address.
  std::vector<Chunk<E> *> chunk_of(ctx.script_sections.size());
  std::vector<bool> is_dead(ctx.script_sections.size(), true);

  for (std::unique_ptr<OutputSection<E>> &osec : ctx.osec_pool)
    if (osec->script_cmd != -1)
      chunk_of[osec->script_cmd] = osec.get();

  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk->script_cmd != -1) {
      chunk_of[chunk->script_cmd] = chunk;
      is_dead[chunk->script_cmd] = false;
    }
  }

  u64 dot = 0;
  i64 lma_delta = 0;
  i64 cursor = 0;

  // Executes script commands up to (but not including) index `end`.
  // Commands between section descriptions are location counter
  // manipulations and symbol assignments.
  auto run_until = [&](i64 end) {
    for (; cursor < end; cursor++) {
      ScriptCmd &cmd = ctx.script_sections[cursor];

      if (cmd.kind == ScriptCmd::ASSIGNMENT) {
        if (cmd.name == ".") {
          ScriptEvaluator<E> ev{ctx, nullptr, cmd.loc, dot};
          u64 val = ev.eval(*cmd.value).val;
          if (cmd.op == "+=")
            val += dot;
          dot = val;
        } else {
          ScriptAssignment<E> &a = ctx.script_assignments[cmd.aux];
          a.has_dot = true;
          a.dot_sec = nullptr;
          a.dot_offset = dot;
        }
        continue;
      }

      if (cmd.kind != ScriptCmd::OUTPUT_SECTION)
        continue;

      // An empty section that was dropped from the output still gets
      // an address so that symbols defined in its body are meaningful.
      // (A live section here is a non-allocated one, e.g. .comment;
      // it keeps its null address.)
      if (Chunk<E> *chunk = chunk_of[cursor]) {
        if (is_dead[cursor])
          chunk->shdr.sh_addr = dot;
        continue;
      }

      // If nothing matched the description, no output section was
      // even created. Its body may still define symbols and move the
      // location counter, so execute it here.
      std::unordered_map<std::string_view, u64> local;

      for (ScriptCmd &b : cmd.cmds) {
        if (b.kind != ScriptCmd::ASSIGNMENT)
          continue;

        if (b.name == ".") {
          if (std::optional<u64> v = eval_dot_expr(*b.value, dot, local))
            dot = (b.op == "+=") ? dot + *v : *v;
          continue;
        }

        ScriptAssignment<E> &a = ctx.script_assignments[b.aux];
        a.has_dot = true;
        a.dot_sec = nullptr;
        a.dot_offset = dot;

        if (std::optional<u64> v = eval_dot_expr(*b.value, dot, local))
          local[b.name] = *v;
      }
    }
  };

  auto layout = [&](Chunk<E> *chunk, ScriptCmd *cmd) {
    u64 align = chunk->shdr.sh_addralign;
    u64 addr;

    if (cmd && cmd->addr) {
      ScriptEvaluator<E> ev{ctx, nullptr, cmd->loc, dot};
      addr = ev.eval(*cmd->addr).val;
    } else {
      addr = align_to(dot, align);
    }

    if (cmd && cmd->align) {
      ScriptEvaluator<E> ev{ctx, nullptr, cmd->loc, dot};
      addr = align_to(addr, ev.eval(*cmd->align).val);
    }

    chunk->shdr.sh_addr = addr;

    // The body of a script-described section contains location counter
    // arithmetic whose result depends on the section's address, so its
    // size must be recomputed now that the address is known.
    if (cmd && chunk->to_osec())
      script_compute_section_size(ctx, *chunk->to_osec(), addr);

    if (cmd && cmd->at) {
      ScriptEvaluator<E> ev{ctx, nullptr, cmd->loc, addr};
      chunk->lma = ev.eval(*cmd->at).val;
      lma_delta = chunk->lma - addr;
    } else {
      chunk->lma = addr + lma_delta;
    }

    dot = addr + chunk->shdr.sh_size;
  };

  for (Chunk<E> *chunk : ctx.chunks) {
    if (!(chunk->shdr.sh_flags & SHF_ALLOC))
      continue;

    if (chunk->script_cmd != -1) {
      run_until(chunk->script_cmd);
      cursor = chunk->script_cmd + 1;
      layout(chunk, &ctx.script_sections[chunk->script_cmd]);
    } else {
      // An orphan section is laid out where the section ordering pass
      // placed it
      layout(chunk, nullptr);
    }
  }

  run_until(ctx.script_sections.size());
}

// Creates program headers as described by a PHDRS command. A section
// is assigned to the segments its `:phdr` references name; a section
// without references inherits the assignment of the previous one.
template <typename E>
std::vector<ElfPhdr<E>> create_script_phdrs(Context<E> &ctx) {
  auto get_type = [&](ScriptPhdr &p) -> u32 {
    static constexpr std::pair<std::string_view, u32> types[] = {
      {"PT_NULL", PT_NULL},
      {"PT_LOAD", PT_LOAD},
      {"PT_DYNAMIC", PT_DYNAMIC},
      {"PT_INTERP", PT_INTERP},
      {"PT_NOTE", PT_NOTE},
      {"PT_PHDR", PT_PHDR},
      {"PT_TLS", PT_TLS},
      {"PT_GNU_EH_FRAME", PT_GNU_EH_FRAME},
      {"PT_GNU_STACK", PT_GNU_STACK},
      {"PT_GNU_RELRO", PT_GNU_RELRO},
      {"PT_GNU_PROPERTY", PT_GNU_PROPERTY},
    };
    for (auto [name, val] : types)
      if (p.type == name)
        return val;
    if (std::optional<u64> val = parse_number(p.type))
      return *val;
    Fatal(ctx) << "PHDRS: unsupported segment type: " << p.type;
  };

  std::vector<ElfPhdr<E>> vec(ctx.script_phdrs.size());
  std::vector<bool> is_empty(ctx.script_phdrs.size(), true);

  for (i64 i = 0; i < ctx.script_phdrs.size(); i++) {
    ScriptPhdr &p = ctx.script_phdrs[i];
    memset(&vec[i], 0, sizeof(vec[i]));
    vec[i].p_type = get_type(p);
    vec[i].p_align = (vec[i].p_type == PT_LOAD) ? ctx.page_size : 1;
    if (p.flags)
      vec[i].p_flags = p.flags->value;
  }

  // Resolve segment references. A section without an explicit
  // reference inherits the previous section's segments.
  std::vector<i64> refs;

  auto find_phdr = [&](std::string_view name) -> i64 {
    for (i64 i = 0; i < ctx.script_phdrs.size(); i++)
      if (ctx.script_phdrs[i].name == name)
        return i;
    Fatal(ctx) << "PHDRS: no such segment: " << name;
  };

  for (Chunk<E> *chunk : ctx.chunks) {
    if (!(chunk->shdr.sh_flags & SHF_ALLOC))
      continue;

    if (chunk->script_cmd != -1) {
      ScriptCmd &cmd = ctx.script_sections[chunk->script_cmd];
      if (!cmd.phdr_refs.empty()) {
        refs.clear();
        for (std::string_view name : cmd.phdr_refs)
          refs.push_back(find_phdr(name));
      }
    }

    for (i64 i : refs) {
      ElfPhdr<E> &phdr = vec[i];
      u64 end = chunk->shdr.sh_addr + chunk->shdr.sh_size;

      if (is_empty[i]) {
        is_empty[i] = false;
        phdr.p_offset = chunk->shdr.sh_offset;
        phdr.p_vaddr = chunk->shdr.sh_addr;
        phdr.p_paddr = chunk->lma;

        // FILEHDR/PHDRS extend the segment backward to also cover the
        // ELF header and the program header table
        if (ctx.script_phdrs[i].filehdr || ctx.script_phdrs[i].phdrs) {
          u64 delta = chunk->shdr.sh_offset;
          phdr.p_offset = 0;
          phdr.p_vaddr -= delta;
          phdr.p_paddr -= delta;
        }
      }

      if (chunk->shdr.sh_type != SHT_NOBITS)
        phdr.p_filesz =
          chunk->shdr.sh_offset + chunk->shdr.sh_size - phdr.p_offset;
      phdr.p_memsz = end - phdr.p_vaddr;
      phdr.p_align = std::max<u64>(phdr.p_align, chunk->shdr.sh_addralign);
      if (!ctx.script_phdrs[i].flags)
        phdr.p_flags |= to_phdr_flags(ctx, chunk);
    }
  }
  return vec;
}

//
// --dump-script
//

static std::ostream &operator<<(std::ostream &out, const ScriptExpr &e) {
  switch (e.kind) {
  case ScriptExpr::INT:
    out << "0x" << std::hex << e.value << std::dec;
    break;
  case ScriptExpr::NAME:
    out << e.str;
    break;
  case ScriptExpr::DOT:
    out << ".";
    break;
  case ScriptExpr::UNARY:
    out << "(" << e.str << e.args[0] << ")";
    break;
  case ScriptExpr::BINARY:
    out << "(" << e.args[0] << " " << e.str << " " << e.args[1] << ")";
    break;
  case ScriptExpr::TERNARY:
    out << "(" << e.args[0] << " ? " << e.args[1] << " : " << e.args[2] << ")";
    break;
  case ScriptExpr::FUNC:
    out << e.str << "(";
    for (i64 i = 0; i < e.args.size(); i++)
      out << (i ? ", " : "") << e.args[i];
    out << ")";
    break;
  }
  return out;
}

static std::string_view get_sort_name(ScriptSort sort) {
  switch (sort) {
  case ScriptSort::NONE:
    return "SORT_NONE";
  case ScriptSort::NAME:
    return "SORT_BY_NAME";
  case ScriptSort::ALIGNMENT:
    return "SORT_BY_ALIGNMENT";
  case ScriptSort::INIT_PRIORITY:
    return "SORT_BY_INIT_PRIORITY";
  case ScriptSort::REVERSE:
    return "REVERSE";
  }
  unreachable();
}

static std::ostream &operator<<(std::ostream &out, const ScriptPattern &pat) {
  for (ScriptSort sort : pat.sorts)
    out << get_sort_name(sort) << "(";
  for (std::string_view file : pat.exclude_files)
    out << "EXCLUDE_FILE(" << file << ") ";
  for (i64 i = 0; i < pat.pats.size(); i++)
    out << (i ? " " : "") << pat.pats[i];
  for (i64 i = 0; i < pat.sorts.size(); i++)
    out << ")";
  return out;
}

static void dump_cmd(std::ostream &out, const ScriptCmd &cmd, i64 depth) {
  std::string ind(depth * 2, ' ');
  out << ind;

  switch (cmd.kind) {
  case ScriptCmd::SECTIONS:
    out << "SECTIONS {\n";
    for (const ScriptCmd &c : cmd.cmds)
      dump_cmd(out, c, depth + 1);
    out << ind << "}\n";
    break;
  case ScriptCmd::OUTPUT_SECTION:
  case ScriptCmd::OVERLAY:
    if (cmd.kind == ScriptCmd::OVERLAY)
      out << "OVERLAY";
    else
      out << cmd.name;

    if (cmd.addr)
      out << " " << *cmd.addr;
    if (!cmd.type.empty())
      out << " (" << cmd.type << ")";
    out << " :";
    if (cmd.at)
      out << " AT(" << *cmd.at << ")";
    if (cmd.align)
      out << " ALIGN(" << *cmd.align << ")";
    if (cmd.align_with_input)
      out << " ALIGN_WITH_INPUT";
    if (cmd.subalign)
      out << " SUBALIGN(" << *cmd.subalign << ")";
    if (!cmd.constraint.empty())
      out << " " << cmd.constraint;

    out << " {\n";
    for (const ScriptCmd &c : cmd.cmds)
      dump_cmd(out, c, depth + 1);
    out << ind << "}";

    if (!cmd.region.empty())
      out << " >" << cmd.region;
    if (!cmd.lma_region.empty())
      out << " AT>" << cmd.lma_region;
    for (std::string_view phdr : cmd.phdr_refs)
      out << " :" << phdr;
    if (cmd.fill)
      out << " =" << *cmd.fill;
    out << "\n";
    break;
  case ScriptCmd::INPUT_SECTION:
    if (cmd.keep)
      out << "KEEP(";
    if (!cmd.names.empty()) {
      out << "INPUT_SECTION_FLAGS(";
      for (i64 i = 0; i < cmd.names.size(); i++)
        out << (i ? " & " : "") << cmd.names[i];
      out << ") ";
    }
    for (std::string_view file : cmd.exclude_files)
      out << "EXCLUDE_FILE(" << file << ") ";
    for (ScriptSort sort : cmd.file_sorts)
      out << get_sort_name(sort) << "(";
    out << cmd.file_pat;
    for (i64 i = 0; i < cmd.file_sorts.size(); i++)
      out << ")";
    if (!cmd.pats.empty()) {
      out << "(";
      for (i64 i = 0; i < cmd.pats.size(); i++)
        out << (i ? " " : "") << cmd.pats[i];
      out << ")";
    }
    if (cmd.keep)
      out << ")";
    out << "\n";
    break;
  case ScriptCmd::ASSIGNMENT:
    if (cmd.provide && cmd.hidden)
      out << "PROVIDE_HIDDEN(";
    else if (cmd.provide)
      out << "PROVIDE(";
    else if (cmd.hidden)
      out << "HIDDEN(";
    out << cmd.name << " " << cmd.op << " " << *cmd.value;
    if (cmd.provide || cmd.hidden)
      out << ")";
    out << ";\n";
    break;
  case ScriptCmd::DATA:
    out << cmd.op << "(" << *cmd.value << ")\n";
    break;
  case ScriptCmd::FILL:
    out << "FILL(" << *cmd.value << ")\n";
    break;
  case ScriptCmd::ASCIZ:
    out << "ASCIZ \"" << cmd.msg << "\"\n";
    break;
  case ScriptCmd::CONSTRUCTORS:
    out << "CONSTRUCTORS\n";
    break;
  case ScriptCmd::ASSERT:
    out << "ASSERT(" << *cmd.value << ", \"" << cmd.msg << "\")\n";
    break;
  case ScriptCmd::ENTRY:
    out << "ENTRY(" << cmd.name << ")\n";
    break;
  case ScriptCmd::MEMORY:
    out << "MEMORY {\n";
    for (const ScriptRegion &r : cmd.memory) {
      out << ind << "  " << r.name;
      if (!r.attrs.empty())
        out << " (" << r.attrs << ")";
      out << " : ORIGIN = " << r.origin << ", LENGTH = " << r.length << "\n";
    }
    out << ind << "}\n";
    break;
  case ScriptCmd::PHDRS:
    out << "PHDRS {\n";
    for (const ScriptPhdr &p : cmd.phdrs) {
      out << ind << "  " << p.name << " " << p.type;
      if (p.filehdr)
        out << " FILEHDR";
      if (p.phdrs)
        out << " PHDRS";
      if (p.at)
        out << " AT(" << *p.at << ")";
      if (p.flags)
        out << " FLAGS(" << *p.flags << ")";
      out << ";\n";
    }
    out << ind << "}\n";
    break;
  case ScriptCmd::INSERT:
    out << "INSERT " << (cmd.insert_before ? "BEFORE" : "AFTER") << " "
        << cmd.name << "\n";
    break;
  case ScriptCmd::REGION_ALIAS:
    out << "REGION_ALIAS(" << cmd.name2 << ", " << cmd.name << ")\n";
    break;
  case ScriptCmd::OTHER:
    out << cmd.name;
    if (!cmd.names.empty()) {
      out << "(";
      for (i64 i = 0; i < cmd.names.size(); i++)
        out << (i ? " " : "") << cmd.names[i];
      out << ")";
    }
    out << "\n";
    break;
  }
}

template <typename E>
void Script<E>::dump() {
  for (ScriptCmd &cmd : cmds)
    dump_cmd(std::cout, cmd, 0);
}

//
// Non-script entry points
//

template <typename E>
std::string_view Script<E>::get_script_output_type() {
  if (peek() == "OUTPUT_FORMAT" && peek(1) == "(") {
    if (peek(2) == "elf64-x86-64")
      return X86_64::name;
    if (peek(2) == "elf32-i386")
      return I386::name;
  }

  if ((peek() == "INPUT" || peek() == "GROUP") && peek(1) == "(") {
    i64 i = (peek(2) == "AS_NEEDED" && peek(3) == "(") ? 4 : 2;
    if (!peek(i).empty())
      if (MappedFile *mf2 = resolve_path(peek(i), false))
        return get_machine_type(ctx, rctx, mf2);
  }
  return "";
}

template <typename E>
void Script<E>::read_version_script_commands(std::string_view ver_str,
                                             u16 ver_idx, bool is_global,
                                             bool is_cpp) {
  while (!at_eof() && peek() != "}") {
    if (consume_label("global")) {
      is_global = true;
      continue;
    }

    if (consume_label("local")) {
      is_global = false;
      continue;
    }

    if (consume("extern")) {
      bool is_cpp2;
      if (consume("\"C\"")) {
        is_cpp2 = false;
      } else {
        skip("\"C++\"");
        is_cpp2 = true;
      }

      skip("{");
      read_version_script_commands(ver_str, ver_idx, is_global, is_cpp2);
      skip("}");
      skip(";");
      continue;
    }

    if (peek() == "*") {
      ctx.default_version = (is_global ? ver_idx : (u32)VER_NDX_LOCAL);
    } else if (is_global) {
      ctx.version_patterns.push_back({unquote(peek()), mf->name, ver_str,
                                      ver_idx, is_cpp});
    } else {
      ctx.version_patterns.push_back({unquote(peek()), mf->name, ver_str,
                                      VER_NDX_LOCAL, is_cpp});
    }

    next();
    if (peek() == "}")
      break;
    skip(";");
  }
}

template <typename E>
void Script<E>::read_version_script() {
  u16 next_ver = VER_NDX_LAST_RESERVED + ctx.arg.version_definitions.size() + 1;

  while (!at_eof() && peek() != "}") {
    std::string_view ver_str;
    u16 ver_idx;

    if (peek() == "{") {
      ver_str = "global";
      ver_idx = VER_NDX_GLOBAL;
    } else {
      ver_str = next();
      ver_idx = next_ver++;
      ctx.arg.version_definitions.emplace_back(ver_str);
    }

    skip("{");
    read_version_script_commands(ver_str, ver_idx, true, false);
    skip("}");

    // A version definition may be followed by a predecessor version
    // name (e.g. `VER_1.1 { ... } VER_1.0;`), which we just ignore.
    if (!at_eof() && peek() != ";")
      next();
    skip(";");
  }
}

template <typename E>
void Script<E>::parse_version_script() {
  read_version_script();
  if (!at_eof())
    error(peek(), "trailing garbage token");
}

template <typename E>
void Script<E>::read_dynamic_list_commands(std::vector<DynamicPattern> &result,
                                           bool is_cpp) {
  while (!at_eof() && peek() != "}") {
    if (consume("extern")) {
      bool is_cpp2;
      if (consume("\"C\"")) {
        is_cpp2 = false;
      } else {
        skip("\"C++\"");
        is_cpp2 = true;
      }

      skip("{");
      read_dynamic_list_commands(result, is_cpp2);
      skip("}");
      skip(";");
      continue;
    }

    result.push_back({unquote(next()), "", is_cpp});
    skip(";");
  }
}

template <typename E>
std::vector<DynamicPattern> Script<E>::parse_dynamic_list() {
  std::vector<DynamicPattern> result;

  skip("{");
  read_dynamic_list_commands(result, false);
  skip("}");
  skip(";");

  if (!at_eof())
    error(peek(), "trailing garbage token");

  for (DynamicPattern &p : result)
    p.source = mf->name;
  return result;
}

template <typename E>
std::vector<DynamicPattern>
parse_dynamic_list(Context<E> &ctx, std::string_view path) {
  ReaderContext rctx;
  MappedFile *mf = must_open_file(ctx, std::string(path));
  return Script(ctx, rctx, mf).parse_dynamic_list();
}

using E = MOLD_TARGET;

template class Script<E>;

template
std::vector<DynamicPattern> parse_dynamic_list(Context<E> &, std::string_view);

template void eval_script_commands(Context<E> &);
template void apply_script_discards(Context<E> &);
template void match_script_sections(Context<E> &);
template void match_synthetic_sections(Context<E> &);
template void script_finalize_section(Context<E> &, OutputSection<E> &);
template void script_compute_section_size(Context<E> &, OutputSection<E> &, u64);
template void set_virtual_addresses_by_script(Context<E> &);
template std::vector<ElfPhdr<E>> create_script_phdrs(Context<E> &);

} // namespace mold
