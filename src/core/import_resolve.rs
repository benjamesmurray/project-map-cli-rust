//! Fallback import resolution by dotted-name suffix.
//!
//! Pure: no I/O and no global state, so the result depends only on the import string and the
//! set of file FQNs. All comparisons are on whole dot-separated segments, never raw string
//! suffixes, and every candidate is considered, so the result does not depend on any iteration
//! order.

use std::collections::{BTreeMap, BTreeSet};

/// The outcome of resolving one import against the known file FQNs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuffixResolution {
    /// The single best file FQN; an `Imports` edge is added.
    Unique(String),
    /// Tied best candidates, sorted; no edge, the import stays unresolved.
    Ambiguous(Vec<String>),
    /// No candidate; no edge.
    None,
}

/// A candidate's rank: lower is better. Tier first, then extra leading segments.
type Rank = (u8, usize);

fn rank(import: &[&str], fqn: &[&str]) -> Option<Rank> {
    if fqn.ends_with(import) {
        return Some((1, fqn.len() - import.len()));
    }
    if import.starts_with(fqn) {
        return Some((2, import.len() - fqn.len()));
    }
    if import.len() >= 2 && fqn.len() >= 2 && import[import.len() - 2..] == fqn[fqn.len() - 2..] {
        return Some((3, import.len() - 2 + fqn.len() - 2));
    }
    None
}

/// The file FQNs indexed by first and last segment, so one import is compared with the few
/// FQNs that can match it instead of with every file. Candidates are visited in sorted order.
pub struct SuffixIndex {
    /// last segment -> FQNs ending in it (tiers 1 and 3 need an equal last segment)
    by_last: BTreeMap<String, Vec<String>>,
    /// first segment -> FQNs starting with it (tier 2 needs an equal first segment)
    by_first: BTreeMap<String, Vec<String>>,
}

impl SuffixIndex {
    pub fn new(fqns: &BTreeSet<String>) -> Self {
        let mut by_last: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut by_first: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for fqn in fqns {
            let first = fqn.split('.').next().unwrap_or_default();
            let last = fqn.rsplit('.').next().unwrap_or_default();
            by_last
                .entry(last.to_string())
                .or_default()
                .push(fqn.clone());
            by_first
                .entry(first.to_string())
                .or_default()
                .push(fqn.clone());
        }
        Self { by_last, by_first }
    }

    /// Picks the file FQN an import refers to when no exact or path match exists.
    ///
    /// Tiers, best first, all on whole dot-separated segments:
    /// 1. the FQN ends with the import's segments;
    /// 2. the import starts with the FQN's segments (a member imported from a module);
    /// 3. both have at least two segments and their last two segments are equal.
    ///
    /// Within a tier, fewer extra leading segments wins. A remaining tie is `Ambiguous`.
    pub fn resolve(&self, import: &str) -> SuffixResolution {
        let import_segs: Vec<&str> = import.split('.').collect();
        if import.is_empty() || import_segs.iter().any(|s| s.is_empty()) {
            return SuffixResolution::None;
        }
        let (first, last) = (import_segs[0], import_segs[import_segs.len() - 1]);
        // A candidate may be in both lists; ranking it twice is harmless because winners
        // are deduplicated below.
        let candidates = self
            .by_last
            .get(last)
            .into_iter()
            .chain(self.by_first.get(first))
            .flatten();
        let mut best: Option<Rank> = None;
        let mut winners: BTreeSet<&String> = BTreeSet::new();
        for fqn in candidates {
            let segs: Vec<&str> = fqn.split('.').collect();
            let Some(r) = rank(&import_segs, &segs) else {
                continue;
            };
            match best {
                Some(b) if r > b => {}
                Some(b) if r == b => {
                    winners.insert(fqn);
                }
                _ => {
                    best = Some(r);
                    winners.clear();
                    winners.insert(fqn);
                }
            }
        }
        match winners.len() {
            0 => SuffixResolution::None,
            1 => SuffixResolution::Unique(winners.into_iter().next().unwrap().clone()),
            _ => SuffixResolution::Ambiguous(winners.into_iter().cloned().collect()),
        }
    }
}

/// One-off form of [`SuffixIndex::resolve`]; build a [`SuffixIndex`] to resolve many imports.
pub fn resolve_by_suffix(import: &str, fqns: &BTreeSet<String>) -> SuffixResolution {
    SuffixIndex::new(fqns).resolve(import)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn unique(s: &str) -> SuffixResolution {
        SuffixResolution::Unique(s.to_string())
    }

    #[test]
    fn tier_one_fqn_ends_with_import() {
        let fqns = set(&["src.app.config", "src.app.service"]);
        assert_eq!(
            resolve_by_suffix("app.config", &fqns),
            unique("src.app.config")
        );
    }

    #[test]
    fn tier_two_import_ends_with_fqn() {
        let fqns = set(&["pkg.mod", "other.thing"]);
        assert_eq!(resolve_by_suffix("pkg.mod.Thing", &fqns), unique("pkg.mod"));
    }

    #[test]
    fn tier_three_last_two_segments_equal() {
        let fqns = set(&["vendor.lib.mod", "vendor.lib.other"]);
        assert_eq!(
            resolve_by_suffix("x.lib.mod", &fqns),
            unique("vendor.lib.mod")
        );
    }

    #[test]
    fn tier_three_needs_two_segments_on_both_sides() {
        let fqns = set(&["a.b.mod"]);
        assert_eq!(
            resolve_by_suffix("z.y.mod.q", &fqns),
            SuffixResolution::None
        );
        assert_eq!(resolve_by_suffix("mod", &fqns), unique("a.b.mod"));
    }

    #[test]
    fn better_tier_beats_fewer_leading_segments() {
        // Tier 1 with three extra segments beats tier 2 with none.
        let fqns = set(&["a.b.c.app.config", "app"]);
        assert_eq!(
            resolve_by_suffix("app.config", &fqns),
            unique("a.b.c.app.config")
        );
    }

    #[test]
    fn fewer_leading_segments_wins_within_a_tier() {
        let fqns = set(&["src.app.config", "tests.fixtures.src.app.config"]);
        assert_eq!(
            resolve_by_suffix("app.config", &fqns),
            unique("src.app.config")
        );
    }

    #[test]
    fn exact_tie_is_ambiguous_with_sorted_candidates() {
        let fqns = set(&["b.app.config", "a.app.config", "deep.er.app.config"]);
        assert_eq!(
            resolve_by_suffix("app.config", &fqns),
            SuffixResolution::Ambiguous(vec!["a.app.config".into(), "b.app.config".into()])
        );
    }

    #[test]
    fn raw_substring_is_not_a_match() {
        let fqns = set(&["myredis", "pkg.myredis"]);
        assert_eq!(resolve_by_suffix("redis", &fqns), SuffixResolution::None);
        assert_eq!(
            resolve_by_suffix("pkg.redis", &fqns),
            SuffixResolution::None
        );
    }

    #[test]
    fn empty_and_malformed_imports_do_not_match() {
        let fqns = set(&["a", "a.b"]);
        assert_eq!(resolve_by_suffix("", &fqns), SuffixResolution::None);
        assert_eq!(resolve_by_suffix(".a", &fqns), SuffixResolution::None);
        assert_eq!(resolve_by_suffix("a.", &fqns), SuffixResolution::None);
    }

    #[test]
    fn single_segment_import() {
        let fqns = set(&["src.util", "other"]);
        assert_eq!(resolve_by_suffix("util", &fqns), unique("src.util"));
        assert_eq!(resolve_by_suffix("missing", &fqns), SuffixResolution::None);
    }

    #[test]
    fn fqn_equal_to_import_is_unique() {
        let fqns = set(&["app.config", "src.app.config"]);
        assert_eq!(resolve_by_suffix("app.config", &fqns), unique("app.config"));
    }

    #[test]
    fn result_does_not_depend_on_insertion_order() {
        let a = set(&["x.m", "y.m", "z.w.m"]);
        let b = set(&["z.w.m", "y.m", "x.m"]);
        assert_eq!(resolve_by_suffix("m", &a), resolve_by_suffix("m", &b));
    }

    /// The indexed lookup agrees with ranking every FQN, over every short name on a small alphabet.
    #[test]
    fn index_agrees_with_ranking_every_fqn() {
        fn names(max_len: usize) -> Vec<String> {
            let mut all = Vec::new();
            let mut layer: Vec<String> = vec![String::new()];
            for _ in 0..max_len {
                layer = layer
                    .iter()
                    .flat_map(|p| {
                        ["a", "b", "c"].map(|seg| {
                            if p.is_empty() {
                                seg.to_string()
                            } else {
                                format!("{p}.{seg}")
                            }
                        })
                    })
                    .collect();
                all.extend(layer.iter().cloned());
            }
            all
        }
        let fqns: BTreeSet<String> = names(3).into_iter().step_by(2).collect();
        let index = SuffixIndex::new(&fqns);
        for import in names(4) {
            let segs: Vec<&str> = import.split('.').collect();
            let mut best: Option<Rank> = None;
            let mut winners: Vec<String> = Vec::new();
            for fqn in &fqns {
                let f: Vec<&str> = fqn.split('.').collect();
                if let Some(r) = rank(&segs, &f) {
                    if best.is_none_or(|b| r < b) {
                        best = Some(r);
                        winners.clear();
                    }
                    if best == Some(r) {
                        winners.push(fqn.clone());
                    }
                }
            }
            let expected = match winners.len() {
                0 => SuffixResolution::None,
                1 => SuffixResolution::Unique(winners.remove(0)),
                _ => SuffixResolution::Ambiguous(winners),
            };
            assert_eq!(index.resolve(&import), expected, "{import}");
        }
    }
}
