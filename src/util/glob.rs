//! Glob matching for symbol name patterns in version scripts and dynamic
//! lists.
//!
//! Exact, prefix and suffix patterns are matched directly, simple
//! substring patterns are combined into an Aho-Corasick automaton, and the
//! remaining patterns use the non-recursive algorithm described at
//! <https://research.swtch.com/glob>. If there are many such patterns, a
//! bit-parallel NFA matches them all at once.

use std::collections::VecDeque;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Str,
    Star,
    Question,
    Bracket,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    str: Vec<u8>,
    chars: Box<[bool; 256]>,
}

impl Token {
    fn new(kind: Kind) -> Self {
        Token {
            kind,
            str: Vec::new(),
            chars: Box::new([false; 256]),
        }
    }
}

#[derive(Debug)]
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
                    // Bracket expressions: [abc], [a-z], [!a-z] and [^a-z].
                    // Both `!` and `^` negate; `!` is the POSIX convention
                    // and `^` is kept for backward compatibility.
                    let mut tok = Token::new(Kind::Bracket);
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
                                tok.chars[i as usize] = true;
                            }
                        } else {
                            tok.chars[pat[0] as usize] = true;
                            pat = &pat[1..];
                        }
                    }

                    if !closed {
                        return None;
                    }
                    if negate {
                        for flag in tok.chars.iter_mut() {
                            *flag = !*flag;
                        }
                    }
                    tokens.push(tok);
                }
                b'?' => tokens.push(Token::new(Kind::Question)),
                b'*' => {
                    if tokens.last().is_none_or(|t| t.kind != Kind::Star) {
                        tokens.push(Token::new(Kind::Star));
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
        if let Some(last) = self.tokens.last() {
            if last.kind == Kind::Str && !s.ends_with(&last.str) {
                return false;
            }
        }

        let mut x = 0;
        let mut y = 0;
        let mut next: Option<(usize, usize)> = None;

        while x < s.len() || y < self.tokens.len() {
            if y < self.tokens.len() {
                let tok = &self.tokens[y];
                match tok.kind {
                    Kind::Str => {
                        if s[x..].starts_with(&tok.str) {
                            x += tok.str.len();
                            y += 1;
                            continue;
                        }
                    }
                    Kind::Star => {
                        next = Some((x + 1, y));
                        y += 1;
                        if let Some(tok) = self.tokens.get(y).filter(|t| t.kind == Kind::Str) {
                            let Some(pos) = find(&s[x..], &tok.str) else {
                                return false;
                            };
                            let pos = x + pos;
                            next = Some((pos + 1, y - 1));
                            x = pos + tok.str.len();
                            y += 1;
                        }
                        continue;
                    }
                    Kind::Question => {
                        if x < s.len() {
                            x += 1;
                            y += 1;
                            continue;
                        }
                    }
                    Kind::Bracket => {
                        if x < s.len() && tok.chars[s[x] as usize] {
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
    if tokens.last().is_none_or(|t| t.kind != Kind::Str) {
        tokens.push(Token::new(Kind::Str));
    }
    tokens.last_mut().unwrap().str.push(c);
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    let end = haystack.len().checked_sub(needle.len())? + 1;
    let mut pos = 0;
    while pos < end {
        pos += memchr::memchr(needle[0], &haystack[pos..end])?;
        if haystack[pos..].starts_with(needle) {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

/// A bit-parallel NFA that matches many glob patterns at once. Each NFA
/// state is one bit, so a byte of input advances all patterns with a few
/// word operations.
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
                match tok.kind {
                    Kind::Str => num_states += tok.str.len(),
                    Kind::Star => {}
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
                match tok.kind {
                    Kind::Str => {
                        for &c in &tok.str {
                            state += 1;
                            nfa.char_masks[c as usize * num_words + state / 64] |=
                                1 << (state % 64);
                        }
                    }
                    Kind::Star => set_bit(&mut nfa.star_states, state),
                    Kind::Question => {
                        state += 1;
                        for c in 0..256 {
                            nfa.char_masks[c * num_words + state / 64] |= 1 << (state % 64);
                        }
                    }
                    Kind::Bracket => {
                        state += 1;
                        for c in 0..256 {
                            if tok.chars[c] {
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
#[derive(Debug, Default)]
struct AhoCorasick {
    root_children: Vec<i32>,
    nodes: Vec<TrieNode>,
}

#[derive(Debug)]
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
        if self.nodes.is_empty() {
            self.root_children = vec![-1; 256];
            self.nodes.push(TrieNode::default());
        }

        // "foo" is handled as "\0foo\0", "*foo" as "foo\0", "foo*" as
        // "\0foo" and "*foo*" as "foo": NUL marks the beginning or end of
        // the input.
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

        // Suffix links refer to nodes at a smaller depth, so build them
        // breadth-first.
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

    fn find(&self, s: &[u8]) -> i64 {
        if self.nodes.is_empty() {
            return -1;
        }

        let mut idx = 0;
        let mut value = -1;
        let mut walk = |c: u8| {
            let mut j = idx;
            while j != -1 {
                let child = self.find_child(j, c);
                if child != -1 {
                    idx = child;
                    value = value.max(self.nodes[child as usize].value);
                    return;
                }
                j = self.nodes[j as usize].suffix_link;
            }
            idx = 0;
        };

        walk(0);
        for &c in s {
            walk(c);
        }
        walk(0);
        value
    }
}

#[derive(Debug)]
struct Literal {
    pat: Vec<u8>,
    value: i64,
}

/// A set of glob patterns, each associated with a value. Looking up a
/// string returns the largest value among the matching patterns, or -1.
#[derive(Debug, Default)]
pub struct Glob {
    match_all: i64,
    exacts: Vec<Literal>,
    prefixes: Vec<Literal>,
    suffixes: Vec<Literal>,
    patterns: Vec<Pattern>,
    aho_corasick: AhoCorasick,
    compiled: OnceLock<Compiled>,
    is_empty: bool,
}

#[derive(Debug)]
struct Compiled {
    exacts: Vec<Literal>,
    nfa: Nfa,
    patterns: Vec<Pattern>,
    aho_corasick: AhoCorasick,
}

fn is_literal(pat: &[u8]) -> bool {
    !pat.iter().any(|&c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
}

impl Glob {
    pub fn new() -> Self {
        Glob {
            match_all: -1,
            is_empty: true,
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty
    }

    /// Adds a pattern. Returns false if the pattern is malformed.
    pub fn add(&mut self, pat: &[u8], value: i64) -> bool {
        debug_assert!(value >= 0);
        debug_assert!(self.compiled.get().is_none());
        self.is_empty = false;

        // Match-all, exact, prefix and suffix patterns are handled with
        // plain string comparisons, which are much cheaper than the
        // general matchers.
        if pat == b"*" {
            self.match_all = self.match_all.max(value);
            return true;
        }
        if is_literal(pat) {
            self.exacts.push(Literal {
                pat: pat.to_vec(),
                value,
            });
            return true;
        }
        if let Some(prefix) = pat.strip_suffix(b"*").filter(|p| is_literal(p)) {
            self.prefixes.push(Literal {
                pat: prefix.to_vec(),
                value,
            });
            return true;
        }
        if let Some(suffix) = pat.strip_prefix(b"*").filter(|p| is_literal(p)) {
            self.suffixes.push(Literal {
                pat: suffix.to_vec(),
                value,
            });
            return true;
        }
        if AhoCorasick::can_handle(pat) {
            self.aho_corasick.add(pat, value);
            return true;
        }
        match Pattern::compile(pat, value) {
            Some(pattern) => {
                self.patterns.push(pattern);
                true
            }
            None => false,
        }
    }

    fn compiled(&self) -> &Compiled {
        self.compiled.get_or_init(|| {
            // If the same name was added more than once, keep only the
            // entry with the largest value, as find() returns the largest
            // match.
            let mut exacts: Vec<Literal> = self
                .exacts
                .iter()
                .map(|l| Literal {
                    pat: l.pat.clone(),
                    value: l.value,
                })
                .collect();
            exacts.sort_by(|a, b| a.pat.cmp(&b.pat).then(b.value.cmp(&a.value)));
            exacts.dedup_by(|a, b| a.pat == b.pat);

            let mut patterns: Vec<Pattern> = self
                .patterns
                .iter()
                .map(|p| Pattern {
                    tokens: p.tokens.clone(),
                    value: p.value,
                })
                .collect();
            let nfa = if patterns.len() >= 64 {
                let nfa = Nfa::compile(&patterns);
                patterns.clear();
                nfa
            } else {
                Nfa::default()
            };

            let mut aho_corasick = AhoCorasick {
                root_children: self.aho_corasick.root_children.clone(),
                nodes: self
                    .aho_corasick
                    .nodes
                    .iter()
                    .map(|n| TrieNode {
                        value: n.value,
                        suffix_link: n.suffix_link,
                        first_child: n.first_child,
                        next_sibling: n.next_sibling,
                        ch: n.ch,
                    })
                    .collect(),
            };
            aho_corasick.compile();

            Compiled {
                exacts,
                nfa,
                patterns,
                aho_corasick,
            }
        })
    }

    /// Returns the largest value of a matching pattern, or -1 if none match.
    pub fn find(&self, s: &[u8]) -> i64 {
        let compiled = self.compiled();
        let mut value = self.match_all;

        if let Ok(i) = compiled
            .exacts
            .binary_search_by(|l| l.pat.as_slice().cmp(s))
        {
            value = value.max(compiled.exacts[i].value);
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
        if !compiled.nfa.is_empty() {
            value = value.max(compiled.nfa.matches(s));
        }
        for p in &compiled.patterns {
            if value < p.value && p.matches(s) {
                value = p.value;
            }
        }
        value.max(compiled.aho_corasick.find(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pats: &[&str]) -> Glob {
        let mut g = Glob::new();
        for (i, p) in pats.iter().enumerate() {
            assert!(g.add(p.as_bytes(), i as i64));
        }
        g
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
