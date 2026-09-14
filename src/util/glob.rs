//! This file implements the glob matcher used for symbol name patterns.
//! Exact, prefix and suffix patterns are matched directly, and simple
//! substring patterns are combined into an Aho-Corasick matcher. The
//! remaining patterns are matched with the non-recursive algorithm described
//! at https://research.swtch.com/glob. If there are many such patterns, a
//! bit-parallel NFA matches them together.

//! This file implements the Aho-Corasick algorithm to search multiple
//! strings within an input string simultaneously. It is essentially a
//! trie with additional links. For details, see
//! https://en.wikipedia.org/wiki/Aho-Corasick_algorithm.
//!
//! We use it for simple glob patterns in version scripts or dynamic
//! list files. Here are some examples of glob patterns:
//!
//!    qt_private_api_tag*
//!    *16QAccessibleCache*
//!    *32QAbstractFileIconProviderPrivate*
//!    *17QPixmapIconEngine*
//!
//! Aho-Corasick can do only substring search, so it cannot handle
//! complex glob patterns such as `*foo*bar*`. We handle such patterns
//! with the [`Glob`] type.

use std::collections::VecDeque;

#[derive(Clone, Debug)]
enum Token {
    Str(Vec<u8>),
    Star,
    Question,
    Bracket(Box<[bool; 256]>),
}

#[derive(Clone, Debug)]
struct Pattern {
    tokens: Vec<Token>,
    value: i64,
}

impl Pattern {
    fn compile(mut pat: &[u8], value: i64) -> Option<Pattern> {
        let mut tokens: Vec<Token> = Vec::new();

        while let Some((&c, rest)) = pat.split_first() {
            pat = rest;
            match c {
                b'[' => {
                    // Here are a few bracket pattern examples:
                    //
                    // [abc]: a, b or c
                    // [$\]!]: $, ] or !
                    // [a-czg-i]: a, b, c, z, g, h, or i
                    // [!a-z]: Any character except lowercase letters
                    //
                    // Both `!` and `^` are accepted as negation markers. `!` is the
                    // POSIX/shell convention used by other linkers. `^` was mold's
                    // original syntax and is kept for backward compatibility.
                    let mut chars = Box::new([false; 256]);
                    let mut negate = false;
                    let mut closed = false;

                    if let Some((&b'!' | &b'^', rest)) = pat.split_first() {
                        negate = true;
                        pat = rest;
                    }

                    while let Some((&c, rest)) = pat.split_first() {
                        if c == b']' {
                            pat = rest;
                            closed = true;
                            break;
                        }

                        if c == b'\\' {
                            pat = rest;
                            if pat.is_empty() {
                                return None;
                            }
                        }

                        if pat.len() >= 3 && pat[1] == b'-' {
                            let start = pat[0];
                            let mut end = pat[2];
                            pat = &pat[3..];
                            if end == b'\\' {
                                end = *pat.first()?;
                                pat = &pat[1..];
                            }
                            if end < start {
                                return None;
                            }
                            for i in start..=end {
                                chars[i as usize] = true;
                            }
                        } else {
                            chars[pat[0] as usize] = true;
                            pat = &pat[1..];
                        }
                    }

                    if !closed {
                        return None;
                    }
                    if negate {
                        for flag in chars.iter_mut() {
                            *flag = !*flag;
                        }
                    }
                    tokens.push(Token::Bracket(chars));
                }
                b'?' => tokens.push(Token::Question),
                b'*' => {
                    if !matches!(tokens.last(), Some(Token::Star)) {
                        tokens.push(Token::Star);
                    }
                }
                b'\\' => {
                    let (&escaped, rest) = pat.split_first()?;
                    pat = rest;
                    push_char(&mut tokens, escaped);
                }
                _ => push_char(&mut tokens, c),
            }
        }
        Some(Pattern { tokens, value })
    }

    fn matches(&self, s: &[u8]) -> bool {
        if let Some(Token::Str(suffix)) = self.tokens.last() {
            if !s.ends_with(suffix) {
                return false;
            }
        }

        let mut x = 0;
        let mut y = 0;
        let mut next: Option<(usize, usize)> = None;

        while x < s.len() || y < self.tokens.len() {
            if y < self.tokens.len() {
                let tok = &self.tokens[y];
                match tok {
                    Token::Str(literal) => {
                        if s[x..].starts_with(literal) {
                            x += literal.len();
                            y += 1;
                            continue;
                        }
                    }
                    Token::Star => {
                        next = Some((x + 1, y));
                        y += 1;
                        if let Some(Token::Str(literal)) = self.tokens.get(y) {
                            let Some(pos) = memchr::memmem::find(&s[x..], literal) else {
                                return false;
                            };
                            let pos = x + pos;
                            next = Some((pos + 1, y - 1));
                            x = pos + literal.len();
                            y += 1;
                        }
                        continue;
                    }
                    Token::Question => {
                        if x < s.len() {
                            x += 1;
                            y += 1;
                            continue;
                        }
                    }
                    Token::Bracket(chars) => {
                        if x < s.len() && chars[s[x] as usize] {
                            x += 1;
                            y += 1;
                            continue;
                        }
                    }
                }
            }

            // Retry the last star after assigning one more input byte to it.
            match next {
                Some((nx, ny)) if nx <= s.len() => {
                    x = nx;
                    y = ny;
                }
                _ => return false,
            }
        }
        true
    }
}

fn push_char(tokens: &mut Vec<Token>, c: u8) {
    if let Some(Token::Str(literal)) = tokens.last_mut() {
        literal.push(c);
    } else {
        tokens.push(Token::Str(vec![c]));
    }
}

// Nfa matches many glob patterns in parallel by representing each state
// with one bit. It is used only for large pattern sets; matching individual
// patterns is faster when there are only a few of them.
#[derive(Debug, Default)]
struct Nfa {
    initial_states: Vec<u64>,
    star_states: Vec<u64>,
    accept_states: Vec<u64>,
    char_masks: Vec<u64>,
    values: Vec<i64>,
}

impl Nfa {
    fn compile(patterns: &[Pattern]) -> Nfa {
        let mut num_states = 0;
        for pattern in patterns {
            num_states += 1;
            for tok in &pattern.tokens {
                match tok {
                    Token::Str(literal) => num_states += literal.len(),
                    Token::Star => {}
                    _ => num_states += 1,
                }
            }
        }

        let num_words = num_states.div_ceil(64);
        let mut nfa = Nfa {
            initial_states: vec![0; num_words],
            star_states: vec![0; num_words],
            accept_states: vec![0; num_words],
            char_masks: vec![0; 256 * num_words],
            values: vec![-1; num_states],
        };

        let set_bit = |vec: &mut [u64], pos: usize| vec[pos / 64] |= 1 << (pos % 64);
        let mut state = 0;

        for pattern in patterns {
            set_bit(&mut nfa.initial_states, state);
            for tok in &pattern.tokens {
                match tok {
                    Token::Str(literal) => {
                        for &c in literal {
                            state += 1;
                            nfa.char_masks[c as usize * num_words + state / 64] |=
                                1 << (state % 64);
                        }
                    }
                    Token::Star => set_bit(&mut nfa.star_states, state),
                    Token::Question => {
                        state += 1;
                        for c in 0..256 {
                            nfa.char_masks[c * num_words + state / 64] |= 1 << (state % 64);
                        }
                    }
                    Token::Bracket(chars) => {
                        state += 1;
                        for c in 0..256 {
                            if chars[c] {
                                nfa.char_masks[c * num_words + state / 64] |= 1 << (state % 64);
                            }
                        }
                    }
                }
            }
            set_bit(&mut nfa.accept_states, state);
            nfa.values[state] = pattern.value;
            state += 1;
        }
        nfa
    }

    fn is_empty(&self) -> bool {
        self.initial_states.is_empty()
    }

    fn matches(&self, s: &[u8]) -> i64 {
        let num_words = self.initial_states.len();
        let mut states = self.initial_states.clone();

        for &c in s {
            let mask = &self.char_masks[c as usize * num_words..][..num_words];
            let mut carry = 0;
            for i in 0..num_words {
                let old = states[i];
                let next = (old << 1) | carry;
                states[i] = (old & self.star_states[i]) | (next & mask[i]);
                carry = old >> 63;
            }
        }

        let mut value = -1;
        for (i, &state) in states.iter().enumerate().take(num_words) {
            let mut word = state & self.accept_states[i];
            while word != 0 {
                let bit = word.trailing_zeros() as usize;
                value = value.max(self.values[i * 64 + bit]);
                word &= word - 1;
            }
        }
        value
    }
}

/// An Aho-Corasick automaton for patterns that are plain substring
/// searches, such as `*foo*`, `foo*` or `*foo`. Anchors are represented by
/// a NUL byte at the beginning or end of the pattern.
#[derive(Clone, Debug, Default)]
struct AhoCorasick {
    // Most trie nodes have only one child. The root uses a dense table because
    // it is visited for almost every input byte; other edges are stored sparsely.
    root_children: Vec<i32>,
    nodes: Vec<TrieNode>,
    max_value: i64,
}

#[derive(Clone, Debug)]
struct TrieNode {
    value: i64,
    suffix_link: i32,
    first_child: i32,
    next_sibling: i32,
    ch: u8,
}

impl Default for TrieNode {
    fn default() -> Self {
        TrieNode {
            value: -1,
            suffix_link: -1,
            first_child: -1,
            next_sibling: -1,
            ch: 0,
        }
    }
}

impl AhoCorasick {
    fn can_handle(pat: &[u8]) -> bool {
        let pat = pat.strip_prefix(b"*").unwrap_or(pat);
        let pat = pat.strip_suffix(b"*").unwrap_or(pat);
        !pat.is_empty() && !pat.iter().any(|&c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
    }

    fn find_child(&self, node: i32, ch: u8) -> i32 {
        if node == 0 {
            return self.root_children[ch as usize];
        }
        let mut i = self.nodes[node as usize].first_child;
        while i != -1 {
            if self.nodes[i as usize].ch == ch {
                return i;
            }
            i = self.nodes[i as usize].next_sibling;
        }
        -1
    }

    fn add_child(&mut self, node: i32, ch: u8) -> i32 {
        let child = self.find_child(node, ch);
        if child != -1 {
            return child;
        }
        let child = self.nodes.len() as i32;
        let sibling = self.nodes[node as usize].first_child;
        self.nodes.push(TrieNode {
            next_sibling: sibling,
            ch,
            ..TrieNode::default()
        });
        self.nodes[node as usize].first_child = child;
        if node == 0 {
            self.root_children[ch as usize] = child;
        }
        child
    }

    fn add(&mut self, pat: &[u8], value: i64) {
        debug_assert!(Self::can_handle(pat));
        self.max_value = self.max_value.max(value);
        if self.nodes.is_empty() {
            self.root_children = vec![-1; 256];
            self.nodes.push(TrieNode::default());
        }

        // We handle "foo" as if "\0foo\0", "*foo" as if "foo\0", "foo*" as
        // if "\0foo", and "*foo*" as if "foo". Aho-Corasick can do only
        // substring matching, so we use \0 as a beginning/end-of-string
        // markers.
        let mut idx = 0;
        if !pat.starts_with(b"*") {
            idx = self.add_child(idx, 0);
        }
        for &c in pat {
            if c != b'*' {
                idx = self.add_child(idx, c);
            }
        }
        if !pat.ends_with(b"*") {
            idx = self.add_child(idx, 0);
        }
        let node = &mut self.nodes[idx as usize];
        node.value = node.value.max(value);
    }

    fn compile(&mut self) {
        if self.nodes.is_empty() {
            return;
        }

        // A failure link may refer to any node at the previous depth, so failure
        // links must be constructed breadth-first.
        let mut queue = VecDeque::new();
        let mut child = self.nodes[0].first_child;
        while child != -1 {
            self.nodes[child as usize].suffix_link = 0;
            queue.push_back(child);
            child = self.nodes[child as usize].next_sibling;
        }

        while let Some(idx) = queue.pop_front() {
            let mut child = self.nodes[idx as usize].first_child;
            while child != -1 {
                let ch = self.nodes[child as usize].ch;
                let mut suffix = self.nodes[idx as usize].suffix_link;
                while suffix != 0 && self.find_child(suffix, ch) == -1 {
                    suffix = self.nodes[suffix as usize].suffix_link;
                }
                let next = self.find_child(suffix, ch);
                if next != -1 {
                    suffix = next;
                }
                self.nodes[child as usize].suffix_link = suffix;
                let suffix_value = self.nodes[suffix as usize].value;
                let node = &mut self.nodes[child as usize];
                node.value = node.value.max(suffix_value);
                queue.push_back(child);
                child = self.nodes[child as usize].next_sibling;
            }
        }
    }

    // This runs once per byte while matching symbol names. Keep the state
    // in the caller's registers instead of outlining the loop body.
    #[inline(always)]
    fn walk(&self, c: u8, idx: &mut i32, value: &mut i64) -> i64 {
        let mut j = *idx;
        while j != -1 {
            let child = self.find_child(j, c);
            if child != -1 {
                *idx = child;
                *value = (*value).max(self.nodes[child as usize].value);
                return *value;
            }
            j = self.nodes[j as usize].suffix_link;
        }
        *idx = 0;
        *value
    }

    fn find(&self, s: &[u8]) -> i64 {
        if self.nodes.is_empty() {
            return -1;
        }

        let mut idx = 0;
        let mut value = -1;
        self.walk(0, &mut idx, &mut value);
        for &c in s {
            if self.walk(c, &mut idx, &mut value) == self.max_value {
                return self.max_value;
            }
        }
        self.walk(0, &mut idx, &mut value)
    }
}

#[derive(Clone, Debug)]
struct Literal {
    pat: Vec<u8>,
    value: i64,
}

/// A set of glob patterns, each associated with a value. Looking up a
/// string returns the largest value among the matching patterns, or -1.
#[derive(Debug)]
pub struct Glob {
    // Patterns that need only a literal string comparison are kept out
    // of the automaton-based matchers below, which scan the entire input
    // string per query. Real version scripts consist almost entirely of
    // such patterns (e.g. `local: *;` or `v8dbg_*;`), and we match them
    // against every defined symbol name.
    match_all: i64, // "*"
    max_value: i64,
    exacts: Vec<Literal>,
    prefixes: Vec<Literal>,
    suffixes: Vec<Literal>,
    patterns: Vec<Pattern>, // "foo*bar"
    aho_corasick: AhoCorasick,
    nfa: Nfa,
}

/// Collects patterns before compiling an immutable matcher.
#[derive(Debug, Default)]
pub struct GlobBuilder {
    glob: Glob,
}

fn is_literal(pat: &[u8]) -> bool {
    !pat.iter().any(|&c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
}

impl Default for Glob {
    fn default() -> Self {
        Self {
            match_all: -1,
            max_value: -1,
            exacts: Vec::new(),
            prefixes: Vec::new(),
            suffixes: Vec::new(),
            patterns: Vec::new(),
            aho_corasick: AhoCorasick::default(),
            nfa: Nfa::default(),
        }
    }
}

impl GlobBuilder {
    /// Adds a pattern. Returns false if the pattern is malformed.
    pub fn add(&mut self, pat: &[u8], value: i64) -> bool {
        debug_assert!(value >= 0);
        self.glob.max_value = self.glob.max_value.max(value);

        // Match-all, exact, prefix and suffix patterns are handled with
        // plain string comparisons instead of the matchers below, which
        // have to scan the entire input string on every query.
        if pat == b"*" {
            self.glob.match_all = self.glob.match_all.max(value);
            return true;
        }
        if is_literal(pat) {
            self.glob.exacts.push(Literal {
                pat: pat.to_vec(),
                value,
            });
            return true;
        }
        if let Some(prefix) = pat.strip_suffix(b"*").filter(|p| is_literal(p)) {
            self.glob.prefixes.push(Literal {
                pat: prefix.to_vec(),
                value,
            });
            return true;
        }
        if let Some(suffix) = pat.strip_prefix(b"*").filter(|p| is_literal(p)) {
            self.glob.suffixes.push(Literal {
                pat: suffix.to_vec(),
                value,
            });
            return true;
        }
        // If the pattern requires only a single substring search, the
        // Aho-Corasick algorithm is even faster than our glob matcher.
        if AhoCorasick::can_handle(pat) {
            self.glob.aho_corasick.add(pat, value);
            return true;
        }
        match Pattern::compile(pat, value) {
            Some(pattern) => {
                self.glob.patterns.push(pattern);
                true
            }
            None => false,
        }
    }

    /// Consumes the construction state. The result supports parallel queries.
    pub fn build(self) -> Glob {
        let mut glob = self.glob;
        if glob.match_all == glob.max_value {
            return glob;
        }

        // For duplicate names, retain the largest value.
        glob.exacts
            .sort_by(|a, b| a.pat.cmp(&b.pat).then(b.value.cmp(&a.value)));
        glob.exacts.dedup_by(|a, b| a.pat == b.pat);
        if glob.patterns.len() >= 64 {
            glob.nfa = Nfa::compile(&glob.patterns);
            glob.patterns = Vec::new();
        }
        glob.aho_corasick.compile();
        glob
    }
}

impl Glob {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.max_value < 0
    }

    /// Returns the largest value of a matching pattern, or -1 if none match.
    pub fn find(&self, s: &[u8]) -> i64 {
        let mut value = self.match_all;
        if value == self.max_value {
            return value;
        }

        if let Ok(i) = self.exacts.binary_search_by(|l| l.pat.as_slice().cmp(s)) {
            value = value.max(self.exacts[i].value);
        }
        for p in &self.prefixes {
            if value < p.value && s.starts_with(&p.pat) {
                value = p.value;
            }
        }
        for p in &self.suffixes {
            if value < p.value && s.ends_with(&p.pat) {
                value = p.value;
            }
        }
        if value == self.max_value {
            return value;
        }
        value = value.max(self.aho_corasick.find(s));
        if value == self.max_value {
            return value;
        }
        if !self.nfa.is_empty() {
            value = value.max(self.nfa.matches(s));
        }
        for p in &self.patterns {
            if value < p.value && p.matches(s) {
                value = p.value;
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pats: &[&str]) -> Glob {
        let mut g = GlobBuilder::default();
        for (i, p) in pats.iter().enumerate() {
            assert!(g.add(p.as_bytes(), i as i64));
        }
        g.build()
    }

    #[test]
    fn default_has_no_matches() {
        let g = Glob::default();
        assert!(g.is_empty());
        assert_eq!(g.find(b"missing"), -1);

        let mut g = GlobBuilder::default();
        assert!(g.add(b"present", 7));
        let g = g.build();
        assert!(!g.is_empty());
        assert_eq!(g.find(b"present"), 7);
        assert_eq!(g.find(b"missing"), -1);
    }

    #[test]
    fn shared_priorities_preserve_highest_match() {
        let mut g = GlobBuilder::default();
        for (pattern, priority) in [
            ("*", 0),
            ("*inner*", 2),
            ("prefix*", 2),
            ("*suffix", 2),
            ("exact", 3),
            ("p?efix*", 4),
        ] {
            assert!(g.add(pattern.as_bytes(), priority));
        }
        let g = g.build();
        for (name, expected) in [
            ("none", 0),
            ("hasinnersuffix", 2),
            ("suffix", 2),
            ("exact", 3),
            ("prefixsuffix", 4),
        ] {
            assert_eq!(g.find(name.as_bytes()), expected);
        }
        let mut g = GlobBuilder::default();
        assert!(g.add(b"*inner*", 0));
        assert!(g.add(b"*suffix", 0));
        let g = g.build();
        assert_eq!(g.find(b"innersuffix"), 0);
        assert_eq!(g.find(b"none"), -1);
    }

    #[test]
    fn literals() {
        let g = glob(&["foo", "bar*", "*baz", "*"]);
        assert_eq!(g.find(b"foo"), 3);
        assert_eq!(g.find(b"barx"), 3);
        assert_eq!(g.find(b"xbaz"), 3);
        let g = glob(&["foo", "bar*", "*baz"]);
        assert_eq!(g.find(b"foo"), 0);
        assert_eq!(g.find(b"barx"), 1);
        assert_eq!(g.find(b"xbaz"), 2);
        assert_eq!(g.find(b"nothing"), -1);
    }

    #[test]
    fn complex() {
        let g = glob(&["a*b*c", "x?z", "[a-c]d", "*mid*", "[!x]y"]);
        assert_eq!(g.find(b"axxbyyc"), 0);
        assert_eq!(g.find(b"xyz"), 1);
        assert_eq!(g.find(b"bd"), 2);
        assert_eq!(g.find(b"dd"), -1);
        assert_eq!(g.find(b"leftmidright"), 3);
        assert_eq!(g.find(b"ay"), 4);
        assert_eq!(g.find(b"xy"), -1);
    }

    #[test]
    fn many_patterns_use_nfa() {
        let pats: Vec<String> = (0..100).map(|i| format!("p{i}*q?r")).collect();
        let refs: Vec<&str> = pats.iter().map(String::as_str).collect();
        let g = glob(&refs);
        assert_eq!(g.find(b"p42xxqyr"), 42);
        assert_eq!(g.find(b"p42xxqr"), -1);
    }
}
