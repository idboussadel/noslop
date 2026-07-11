//! `duplicate-code` — cross-file token-clone detection over a suffix array.
//!
//! Pipeline: normalize each file's tokens to a permissive "class" stream →
//! concatenate per language with unique file-boundary sentinels → rank-compress →
//! suffix array (prefix doubling, O(n log n)) → LCP (Kasai) → maximal repeats
//! ≥ `min_tokens` → filter (self-overlap, single-expression abstractions,
//! `skip_local`) → verify down to the requested mode (incl. the consistent-
//! renaming check for `semantic`) → one finding per clone class.
//!
//! The index is built at the most permissive normalization (identifiers, numbers
//! and strings all collapse), so it proposes the widest candidate set; the
//! per-mode verifier then narrows it. This is why one token stream serves all
//! four modes.

use noslop_graph::{
    Confidence, DuplicationConfig, DuplicationMode, Finding, Language, RuleId, Severity, Span, Tok,
    TokKind,
};
use std::path::{Path, PathBuf};

/// A file's duplication tokens, as handed in by the orchestrator.
pub struct FileTokens {
    pub path: PathBuf,
    pub language: Language,
    pub tokens: Vec<Tok>,
}

pub fn run(files: &[FileTokens], cfg: &DuplicationConfig) -> Vec<Finding> {
    if !cfg.enabled {
        return Vec::new();
    }
    let min = cfg.min_tokens.max(1) as usize;

    // A TS↔Python "clone" is coincidence at the token level, so index each
    // language family separately.
    let mut findings = Vec::new();
    let py: Vec<&FileTokens> = files.iter().filter(|f| f.language.is_python()).collect();
    let js: Vec<&FileTokens> = files.iter().filter(|f| !f.language.is_python()).collect();
    for group in [py, js] {
        if !group.is_empty() {
            findings.extend(detect(&group, cfg, min));
        }
    }
    // Stable order: by first-occurrence file then line.
    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.span.start_line.cmp(&b.span.start_line))
    });
    findings
}

/// Maps a position in the concatenated stream back to its token.
#[derive(Clone, Copy)]
struct Loc {
    file: usize, // index into `group`; usize::MAX for a boundary sentinel
    tok: usize,
}

fn detect(group: &[&FileTokens], cfg: &DuplicationConfig, min: usize) -> Vec<Finding> {
    // Build the class-symbol stream + position map, in the *requested mode's*
    // alphabet: two tokens share a symbol iff that mode considers them equal. So
    // the suffix array's repeats are already mode-correct — only `semantic` needs
    // a post-check (identifiers collapse, so consistent renaming must be verified).
    let mut syms: Vec<u64> = Vec::new();
    let mut locs: Vec<Loc> = Vec::new();
    for (fi, f) in group.iter().enumerate() {
        for (ti, t) in f.tokens.iter().enumerate() {
            syms.push(class_symbol(t, cfg.mode));
            locs.push(Loc { file: fi, tok: ti });
        }
        // Unique boundary so no repeat spans two files.
        syms.push(BOUNDARY_BASE + fi as u64);
        locs.push(Loc {
            file: usize::MAX,
            tok: 0,
        });
    }
    if syms.len() <= min {
        return Vec::new();
    }

    let ranks = rank_compress(&syms);
    let sa = suffix_array(&ranks);
    let lcp = kasai_lcp(&ranks, &sa);

    // Group adjacent SA entries whose shared prefix is ≥ min into clone classes.
    let mut findings = Vec::new();
    let mut i = 1;
    while i < sa.len() {
        if lcp[i] < min {
            i += 1;
            continue;
        }
        // Maximal run [start-1 .. j] with every LCP ≥ min; class length = the run min.
        let mut j = i;
        let mut run_len = lcp[i];
        while j + 1 < sa.len() && lcp[j + 1] >= min {
            j += 1;
            run_len = run_len.min(lcp[j]);
        }
        let starts: Vec<usize> = sa[(i - 1)..=j].to_vec();
        // Left-maximality: if every occurrence is preceded by the same symbol, this
        // is a right-shifted copy of a longer clone — skip it so each duplicated
        // block is reported once, not once per suffix.
        if left_maximal(&ranks, &starts) {
            if let Some(finding) = build_class(group, &locs, &starts, run_len, cfg) {
                findings.push(finding);
            }
        }
        i = j + 1;
    }
    findings
}

/// A repeat is left-maximal if its occurrences are not all preceded by the same
/// symbol (or one starts the stream) — otherwise it extends left into a longer clone.
fn left_maximal(ranks: &[u32], starts: &[usize]) -> bool {
    let mut prev: Option<u32> = None;
    for &s in starts {
        if s == 0 {
            return true;
        }
        let before = ranks[s - 1];
        match prev {
            None => prev = Some(before),
            Some(p) if p != before => return true,
            _ => {}
        }
    }
    false
}

/// Turn a set of stream start-positions sharing a `len`-token prefix into a
/// verified clone-class finding, or `None` if it fails a filter.
fn build_class(
    group: &[&FileTokens],
    locs: &[Loc],
    starts: &[usize],
    len: usize,
    cfg: &DuplicationConfig,
) -> Option<Finding> {
    // Resolve stream positions to (file, token-range); drop any spanning a boundary.
    let mut occ: Vec<Occurrence> = Vec::new();
    for &s in starts {
        let start = locs[s];
        if start.file == usize::MAX {
            continue;
        }
        // A shared prefix never spans a boundary (they are unique), but guard anyway.
        let end_pos = s + len - 1;
        if end_pos >= locs.len() || locs[end_pos].file != start.file {
            continue;
        }
        occ.push(Occurrence {
            file: start.file,
            start: start.tok,
            len,
        });
    }
    // Drop occurrences that overlap earlier ones in the same file (self-repeats).
    // `dedup_by(later, earlier)` removes `later` when it overlaps the kept `earlier`.
    occ.sort_by(|a, b| a.file.cmp(&b.file).then(a.start.cmp(&b.start)));
    occ.dedup_by(|later, earlier| {
        later.file == earlier.file && later.start < earlier.start + earlier.len
    });
    if occ.len() < 2 {
        return None;
    }

    // Abstraction filter: a block that isn't at least two statements is likely a
    // repeated call to a shared helper, not copy-paste worth refactoring.
    let first = occ[0];
    let toks = &group[first.file].tokens[first.start..first.start + len];
    if toks.iter().filter(|t| t.stmt_end).count() < 2 {
        return None;
    }

    // `semantic` collapses identifiers in the index, so re-verify consistent
    // renaming here. exact/mild/weak are already exact in their alphabet.
    if cfg.mode == DuplicationMode::Semantic {
        occ.retain(|o| {
            (o.file == first.file && o.start == first.start) || equivalent(group, &first, o, len)
        });
        if occ.len() < 2 {
            return None;
        }
    }

    if cfg.skip_local && same_directory(group, &occ) {
        return None;
    }

    Some(make_finding(group, &occ, len, cfg.mode))
}

#[derive(Clone, Copy)]
struct Occurrence {
    file: usize,
    start: usize,
    len: usize,
}

/// Verify two `semantic`-mode occurrences use a consistent 1-1 identifier
/// renaming (`a→x, b→y`, never `a→x, a→y`). Punct already matched in the index;
/// numbers/strings are wildcards.
fn equivalent(group: &[&FileTokens], a: &Occurrence, b: &Occurrence, len: usize) -> bool {
    let ta = &group[a.file].tokens[a.start..a.start + len];
    let tb = &group[b.file].tokens[b.start..b.start + len];
    let mut fwd: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    let mut rev: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for (x, y) in ta.iter().zip(tb.iter()) {
        if x.kind == TokKind::Ident
            && (*fwd.entry(x.hash).or_insert(y.hash) != y.hash
                || *rev.entry(y.hash).or_insert(x.hash) != x.hash)
        {
            return false;
        }
    }
    true
}

fn make_finding(
    group: &[&FileTokens],
    occ: &[Occurrence],
    len: usize,
    mode: DuplicationMode,
) -> Finding {
    // Deterministic primary occurrence: lowest path, then line.
    let mut sorted = occ.to_vec();
    sorted.sort_by(|a, b| {
        group[a.file]
            .path
            .cmp(&group[b.file].path)
            .then(line_of(group, a).cmp(&line_of(group, b)))
    });
    let primary = sorted[0];
    let fingerprint = class_fingerprint(group, &primary, len);

    let others: Vec<String> = sorted[1..]
        .iter()
        .map(|o| format!("{}:{}", group[o.file].path.display(), line_of(group, o)))
        .collect();

    let start_line = line_of(group, &primary);
    let end_line = group[primary.file].tokens[primary.start + len - 1].line;
    let confidence = match mode {
        DuplicationMode::Exact | DuplicationMode::Mild => Confidence::High,
        DuplicationMode::Weak | DuplicationMode::Semantic => Confidence::Medium,
    };

    Finding {
        rule: RuleId::DuplicateCode,
        severity: Severity::Warn,
        confidence,
        symbol: Some(format!("dup:{fingerprint:08x}")),
        file: group[primary.file].path.clone(),
        span: Span::new(start_line, end_line.max(start_line)),
        message: format!(
            "Duplicated block ({len} tokens, {} occurrences): also at {}.",
            sorted.len(),
            others.join(", ")
        ),
        reason: format!(
            "identical normalized token stream (mode: {})",
            mode.as_str()
        ),
    }
}

fn line_of(group: &[&FileTokens], o: &Occurrence) -> u32 {
    group[o.file].tokens[o.start].line
}

/// 32-bit fingerprint of the class-symbol sequence — stable across line shifts,
/// so a baseline ratchet keys on the clone itself.
fn class_fingerprint(group: &[&FileTokens], o: &Occurrence, len: usize) -> u32 {
    let toks = &group[o.file].tokens[o.start..o.start + len];
    let mut bytes = Vec::with_capacity(len * 8);
    for t in toks {
        // A fixed (semantic) normalization keeps the id stable across modes.
        bytes.extend_from_slice(&class_symbol(t, DuplicationMode::Semantic).to_le_bytes());
    }
    (xxhash_rust::xxh3::xxh3_64(&bytes) & 0xffff_ffff) as u32
}

fn same_directory(group: &[&FileTokens], occ: &[Occurrence]) -> bool {
    let dir = |o: &Occurrence| -> PathBuf {
        group[o.file]
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    let first = dir(&occ[0]);
    occ.iter().all(|o| dir(o) == first)
}

// ── normalization ────────────────────────────────────────────────────────────

const BOUNDARY_BASE: u64 = 1 << 62;

/// The class symbol under a mode: two tokens compare equal iff the mode says so.
/// Punctuation is always exact; each mode additionally collapses numbers, then
/// strings, then identifiers to a per-kind sentinel. Concrete (hashed) symbols
/// carry the high bit so they never collide with the small sentinels or boundaries.
fn class_symbol(t: &Tok, mode: DuplicationMode) -> u64 {
    let collapse_num = !matches!(mode, DuplicationMode::Exact);
    let collapse_str = matches!(mode, DuplicationMode::Weak | DuplicationMode::Semantic);
    let collapse_ident = matches!(mode, DuplicationMode::Semantic);
    match t.kind {
        TokKind::Punct => t.hash | (1 << 63),
        TokKind::Num if collapse_num => 1,
        TokKind::Str if collapse_str => 2,
        TokKind::Ident if collapse_ident => 3,
        _ => t.hash | (1 << 63),
    }
}

// ── suffix array (prefix doubling) + LCP (Kasai) ─────────────────────────────

/// Map arbitrary u64 symbols to dense ranks `0..k` preserving order.
fn rank_compress(syms: &[u64]) -> Vec<u32> {
    let mut sorted: Vec<u64> = syms.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    syms.iter()
        .map(|s| sorted.partition_point(|x| x < s) as u32)
        .collect()
}

/// Suffix array via prefix doubling. O(n log n), no `unsafe`, deterministic.
fn suffix_array(s: &[u32]) -> Vec<usize> {
    let n = s.len();
    let mut sa: Vec<usize> = (0..n).collect();
    let mut rank: Vec<i64> = s.iter().map(|&x| x as i64).collect();
    let mut tmp = vec![0i64; n];
    let mut k = 1;
    while k < n {
        let key = |i: usize| -> (i64, i64) { (rank[i], if i + k < n { rank[i + k] } else { -1 }) };
        sa.sort_by_key(|&a| key(a));
        tmp[sa[0]] = 0;
        for w in 1..n {
            tmp[sa[w]] = tmp[sa[w - 1]] + if key(sa[w - 1]) < key(sa[w]) { 1 } else { 0 };
        }
        rank.copy_from_slice(&tmp);
        if rank[sa[n - 1]] as usize == n - 1 {
            break; // all ranks distinct
        }
        k <<= 1;
    }
    sa
}

/// Kasai's algorithm: `lcp[i]` = longest common prefix of `sa[i-1]` and `sa[i]`.
fn kasai_lcp(s: &[u32], sa: &[usize]) -> Vec<usize> {
    let n = s.len();
    let mut inv = vec![0usize; n];
    for (i, &p) in sa.iter().enumerate() {
        inv[p] = i;
    }
    let mut lcp = vec![0usize; n];
    let mut h = 0;
    for i in 0..n {
        if inv[i] == 0 {
            h = 0;
            continue;
        }
        let j = sa[inv[i] - 1];
        while i + h < n && j + h < n && s[i + h] == s[j + h] {
            h += 1;
        }
        lcp[inv[i]] = h;
        h = h.saturating_sub(1);
    }
    lcp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Naive O(n² log n) suffix array for cross-checking.
    fn naive_sa(s: &[u32]) -> Vec<usize> {
        let mut sa: Vec<usize> = (0..s.len()).collect();
        sa.sort_by(|&a, &b| s[a..].cmp(&s[b..]));
        sa
    }

    fn naive_lcp(s: &[u32], sa: &[usize]) -> Vec<usize> {
        let mut lcp = vec![0usize; s.len()];
        for i in 1..sa.len() {
            let (a, b) = (sa[i - 1], sa[i]);
            let mut k = 0;
            while a + k < s.len() && b + k < s.len() && s[a + k] == s[b + k] {
                k += 1;
            }
            lcp[i] = k;
        }
        lcp
    }

    #[test]
    fn suffix_array_and_lcp_match_naive() {
        // Deterministic pseudo-random small inputs over a tiny alphabet.
        let mut state: u64 = 0x1234_5678;
        for _ in 0..200 {
            let n = (state % 40) as usize + 1;
            let s: Vec<u32> = (0..n)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((state >> 33) % 4) as u32
                })
                .collect();
            let sa = suffix_array(&s);
            assert_eq!(sa, naive_sa(&s), "SA mismatch for {s:?}");
            assert_eq!(
                kasai_lcp(&s, &sa),
                naive_lcp(&s, &sa),
                "LCP mismatch for {s:?}"
            );
        }
    }

    #[test]
    fn rank_compress_is_order_preserving() {
        let r = rank_compress(&[50, 10, 10, 99, 0]);
        assert_eq!(r, vec![2, 1, 1, 3, 0]);
    }
}
