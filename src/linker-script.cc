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

#include <cctype>
#include <charconv>
#include <iostream>

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
void Script<E>::error(std::string_view pos, std::string msg) {
  // The token may have come from a file that the current file INCLUDE'd
  MappedFile *file = mf;
  for (MappedFile *m : files) {
    std::string_view buf = m->get_contents();
    if (buf.data() <= pos.data() && pos.data() < buf.data() + buf.size())
      file = m;
  }

  std::string_view input = file->get_contents();
  std::string_view line = get_line(input, pos.data());

  i64 lineno = 1;
  for (i64 i = 0; input.data() + i < line.data(); i++)
    if (input[i] == '\n')
      lineno++;

  std::string label = file->name + ":" + std::to_string(lineno) + ": ";
  i64 indent = strlen("mold: fatal: ") + label.size();
  i64 column = pos.data() - line.data();

  Fatal(ctx) << label << line << "\n"
             << std::string(indent + column, ' ') << "^ " << msg;
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

// mold cannot evaluate most linker script commands yet. We reject
// anything we cannot faithfully execute so that we never silently
// create an output file that is different from what a script demands.
template <typename E>
void Script<E>::evaluate() {
  for (ScriptCmd &cmd : cmds) {
    // `foo = bar;` and `foo = 0x1000;` are equivalent to --defsym
    if (cmd.kind == ScriptCmd::ASSIGNMENT && !cmd.provide && !cmd.hidden &&
        cmd.op == "=" && cmd.name != ".") {
      if (cmd.value->kind == ScriptExpr::NAME) {
        ctx.arg.defsyms.emplace_back(get_symbol(ctx, cmd.name),
                                     get_symbol(ctx, cmd.value->str));
        continue;
      }
      if (cmd.value->kind == ScriptExpr::INT) {
        ctx.arg.defsyms.emplace_back(get_symbol(ctx, cmd.name),
                                     cmd.value->value);
        continue;
      }
    }
    error(cmd.loc, "this linker script command is not supported yet");
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

} // namespace mold
