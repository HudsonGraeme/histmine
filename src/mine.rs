use crate::tokenize::{Quote, Token, tokenize};
use std::collections::HashMap;

pub struct Mined {
    pub name: String,
    pub body: String,
    pub from_example: String,
    pub try_example: String,
    pub n_entries: usize,
    pub n_params: usize,
    pub saved: i64,
}

const SIM_THRESHOLD: f64 = 0.55;
const MAX_PARAMS: usize = 5;

pub fn mine(items: &[(String, Vec<Token>)], min_uses: usize) -> (Vec<Mined>, usize) {
    let mut heads: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, (_, toks)) in items.iter().enumerate() {
        heads.entry(toks[0].raw.as_str()).or_default().push(i);
    }
    let mut groups: Vec<(&str, Vec<usize>)> = heads.into_iter().collect();
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));

    let mut out = Vec::new();
    let mut withheld = 0;
    let mut used_names: HashMap<String, usize> = HashMap::new();
    for (_, group) in &groups {
        for cluster in cluster(items, group) {
            if cluster.len() < min_uses {
                continue;
            }
            if let Some(m) = generalize(items, &cluster, min_uses, &mut used_names) {
                if looks_secret(&m.body) {
                    withheld += 1;
                } else {
                    out.push(m);
                }
            }
        }
    }
    out.sort_by_key(|m| std::cmp::Reverse(m.saved));
    (out, withheld)
}

fn looks_secret(s: &str) -> bool {
    let lower = s.to_lowercase();
    for kw in [
        "key=",
        "token=",
        "secret=",
        "password=",
        "passwd=",
        "bearer ",
        "authorization:",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
    ] {
        if lower.contains(kw) {
            return true;
        }
    }
    if s.contains("AKIA") {
        return true;
    }
    let mut run = 0;
    for c in s.chars() {
        if c.is_ascii_hexdigit() {
            run += 1;
            if run >= 32 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn cluster(items: &[(String, Vec<Token>)], group: &[usize]) -> Vec<Vec<usize>> {
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for &idx in group {
        let mut best = None;
        let mut best_s = SIM_THRESHOLD;
        for (ci, c) in clusters.iter().enumerate() {
            let s = similarity(&items[idx].1, &items[c[0]].1);
            if s >= best_s {
                best_s = s;
                best = Some(ci);
            }
        }
        match best {
            Some(ci) => clusters[ci].push(idx),
            None => clusters.push(vec![idx]),
        }
    }
    clusters
}

fn similarity(a: &[Token], b: &[Token]) -> f64 {
    let (n, m) = (a.len(), b.len());
    let mut prev = vec![0f64; m + 1];
    let mut cur = vec![0f64; m + 1];
    for i in 1..=n {
        for j in 1..=m {
            let d = prev[j - 1] + tok_score(&a[i - 1], &b[j - 1]);
            cur[j] = d.max(prev[j]).max(cur[j - 1]);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m] / n.max(m) as f64
}

fn tok_score(a: &Token, b: &Token) -> f64 {
    if a.raw == b.raw {
        1.0
    } else if a.op || b.op {
        0.0
    } else {
        0.85 * affix_ratio(&a.raw, &b.raw)
    }
}

fn affix_ratio(a: &str, b: &str) -> f64 {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let lim = ab.len().min(bb.len());
    let mut p = 0;
    while p < lim && ab[p] == bb[p] {
        p += 1;
    }
    let mut s = 0;
    while s < lim - p && ab[ab.len() - 1 - s] == bb[bb.len() - 1 - s] {
        s += 1;
    }
    (p + s) as f64 / ab.len().max(bb.len()) as f64
}

enum Piece {
    Const(String),
    Param(Vec<String>),
    Inner(Vec<InnerPiece>),
}

enum InnerPiece {
    Const(String, bool),
    Param(Vec<String>, Quote, bool),
}

fn generalize(
    items: &[(String, Vec<Token>)],
    cluster: &[usize],
    min_uses: usize,
    used_names: &mut HashMap<String, usize>,
) -> Option<Mined> {
    let mut by_len: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in cluster {
        by_len.entry(items[i].1.len()).or_default().push(i);
    }
    let (len, members) = by_len
        .into_iter()
        .max_by_key(|(l, v)| (v.len(), std::cmp::Reverse(*l)))?;
    if members.len() < min_uses {
        return None;
    }
    let toks: Vec<&[Token]> = members.iter().map(|&i| items[i].1.as_slice()).collect();

    let mut pieces = Vec::new();
    let mut const_words = Vec::new();
    for col in 0..len {
        let first = &toks[0][col];
        if toks.iter().all(|t| t[col].raw == first.raw) {
            if !first.op {
                const_words.push(first.value.clone());
            }
            pieces.push(Piece::Const(first.raw.clone()));
            continue;
        }
        if toks.iter().any(|t| t[col].op) {
            return None;
        }
        if let Some(inner) = try_inner(&toks, col, &mut const_words) {
            pieces.push(Piece::Inner(inner));
            continue;
        }
        let values = toks.iter().map(|t| t[col].value.clone()).collect();
        pieces.push(Piece::Param(values));
    }

    let mut key_to_idx: HashMap<&[String], usize> = HashMap::new();
    let mut argv: Vec<&[String]> = Vec::new();
    let mut body = String::new();
    for (col, p) in pieces.iter().enumerate() {
        if col > 0 && toks[0][col].space_before {
            body.push(' ');
        }
        match p {
            Piece::Const(raw) => body.push_str(raw),
            Piece::Param(values) => {
                let idx = intern(&mut key_to_idx, &mut argv, values);
                body.push_str(&format!("$argv[{}]", idx + 1));
            }
            Piece::Inner(inner) => {
                body.push('"');
                for (j, ip) in inner.iter().enumerate() {
                    match ip {
                        InnerPiece::Const(raw, space) => {
                            if j > 0 && *space {
                                body.push(' ');
                            }
                            body.push_str(raw);
                        }
                        InnerPiece::Param(values, q, space) => {
                            if j > 0 && *space {
                                body.push(' ');
                            }
                            let idx = intern(&mut key_to_idx, &mut argv, values);
                            body.push_str(&match q {
                                Quote::Single => format!("'$argv[{}]'", idx + 1),
                                _ => format!("$argv[{}]", idx + 1),
                            });
                        }
                    }
                }
                body.push('"');
            }
        }
    }

    let n_params = argv.len();
    if n_params == 0 || n_params > MAX_PARAMS || const_words.len() < 2 {
        return None;
    }

    let name = make_name(&const_words, used_names);
    let mut saved = 0i64;
    for (mi, &item_idx) in members.iter().enumerate() {
        let orig = items[item_idx].0.len() as i64;
        let mut inv = name.len() as i64;
        for vals in &argv {
            inv += 1 + quoted_arg(&vals[mi]).len() as i64;
        }
        saved += (orig - inv).max(0);
    }

    let mut try_example = name.clone();
    for vals in &argv {
        try_example.push(' ');
        try_example.push_str(&quoted_arg(&vals[0]));
    }

    Some(Mined {
        name,
        body,
        from_example: items[members[0]].0.clone(),
        try_example,
        n_entries: members.len(),
        n_params,
        saved,
    })
}

fn try_inner(
    toks: &[&[Token]],
    col: usize,
    const_words: &mut Vec<String>,
) -> Option<Vec<InnerPiece>> {
    for t in toks {
        let tok = &t[col];
        if tok.quote != Quote::Double || tok.raw != format!("\"{}\"", tok.value) {
            return None;
        }
    }
    let inners: Vec<Vec<Token>> = toks
        .iter()
        .map(|t| tokenize(&t[col].value))
        .collect::<Option<_>>()?;
    let l = inners[0].len();
    if l < 2 || inners.iter().any(|x| x.len() != l) {
        return None;
    }
    let mut pieces = Vec::new();
    let mut new_words = Vec::new();
    let mut any_const = false;
    for j in 0..l {
        let first = &inners[0][j];
        if inners.iter().all(|x| x[j].raw == first.raw) {
            if first.raw.contains('"') || first.raw.contains('\\') {
                return None;
            }
            any_const = true;
            if !first.op {
                new_words.push(first.value.clone());
            }
            pieces.push(InnerPiece::Const(first.raw.clone(), first.space_before));
        } else {
            if inners.iter().any(|x| x[j].op) {
                return None;
            }
            let values = inners.iter().map(|x| x[j].value.clone()).collect();
            let q = if inners.iter().all(|x| x[j].quote == Quote::Single) {
                Quote::Single
            } else {
                Quote::None
            };
            pieces.push(InnerPiece::Param(values, q, first.space_before));
        }
    }
    if !any_const {
        return None;
    }
    const_words.extend(new_words);
    Some(pieces)
}

fn intern<'a>(
    map: &mut HashMap<&'a [String], usize>,
    argv: &mut Vec<&'a [String]>,
    values: &'a Vec<String>,
) -> usize {
    if let Some(&i) = map.get(values.as_slice()) {
        return i;
    }
    let i = argv.len();
    argv.push(values.as_slice());
    map.insert(values.as_slice(), i);
    i
}

fn make_name(const_words: &[String], used: &mut HashMap<String, usize>) -> String {
    let head = &const_words[0];
    let mut parts: Vec<String> = Vec::new();
    let head_base = head.rsplit('/').next().unwrap_or(head);
    let head_base = head_base.split('.').next().unwrap_or(head_base);
    let head_sane = sanitize(head_base);
    if !head_sane.is_empty() && head_sane.len() <= 16 && !head.contains('=') {
        parts.push(head_sane);
    }
    let mut flags: Vec<String> = Vec::new();
    for w in const_words.iter().skip(1) {
        if parts.len() >= 3 {
            break;
        }
        if w.starts_with('-') {
            let s = sanitize(w.trim_start_matches('-'));
            if !s.is_empty() && s.len() <= 12 {
                flags.push(s);
            }
            continue;
        }
        if w.contains('/') || w.contains('=') || w.contains(':') || w.len() > 16 {
            continue;
        }
        let s = sanitize(w);
        if !s.is_empty() && !parts.contains(&s) {
            parts.push(s);
        }
    }
    while parts.len() < 2 {
        if flags.is_empty() {
            break;
        }
        let f = flags.remove(0);
        if !parts.contains(&f) {
            parts.push(f);
        }
    }
    if parts.is_empty() {
        parts.push("cmd".to_string());
    }
    let mut base = parts.join("_");
    if base == *head {
        base.push_str("_x");
    }
    let n = used.entry(base.clone()).or_insert(0);
    *n += 1;
    if *n == 1 { base } else { format!("{base}_{n}") }
}

fn sanitize(w: &str) -> String {
    let mut out = String::new();
    for c in w.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

pub(crate) fn quoted_arg(v: &str) -> String {
    if !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:@=+,~".contains(c))
    {
        v.to_string()
    } else {
        format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(cmds: &[&str]) -> Vec<(String, Vec<Token>)> {
        cmds.iter()
            .map(|c| (c.to_string(), tokenize(c).unwrap()))
            .collect()
    }

    #[test]
    fn lgg_with_inner_recursion() {
        let it = items(&[
            "ssh prod-3 \"journalctl -u api --since '2 hours ago'\"",
            "ssh prod-7 \"journalctl -u worker --since '10 min ago'\"",
            "ssh prod-1 \"journalctl -u api --since '1 hour ago'\"",
        ]);
        let m = mine(&it, 3).0;
        assert_eq!(m.len(), 1);
        assert_eq!(
            m[0].body,
            "ssh $argv[1] \"journalctl -u $argv[2] --since '$argv[3]'\""
        );
        assert_eq!(m[0].n_params, 3);
    }

    #[test]
    fn correlated_slots_share_one_param() {
        let it = items(&[
            "docker tag app:v1 && docker push app:v1",
            "docker tag app:v2 && docker push app:v2",
            "docker tag app:v3 && docker push app:v3",
        ]);
        let m = mine(&it, 3).0;
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].n_params, 1);
        assert_eq!(m[0].body, "docker tag $argv[1] && docker push $argv[1]");
    }

    #[test]
    fn distinct_subcommands_do_not_merge() {
        let it = items(&[
            "git commit -m one",
            "git commit -m two",
            "git commit -m three",
            "git push origin main",
            "git push origin dev",
            "git push origin main",
        ]);
        let m = mine(&it, 3).0;
        let bodies: Vec<&str> = m.iter().map(|x| x.body.as_str()).collect();
        assert!(bodies.contains(&"git commit -m $argv[1]"));
        assert!(bodies.contains(&"git push origin $argv[1]"));
    }

    #[test]
    fn identical_repeats_yield_nothing() {
        let it = items(&["pnpm run dev", "pnpm run dev", "pnpm run dev"]);
        assert!(mine(&it, 3).0.is_empty());
    }

    #[test]
    fn redirect_renders_without_stray_spaces() {
        let it = items(&[
            "timeout 30 cargo run 2>&1 | grep DEBUG",
            "timeout 45 cargo run 2>&1 | grep TRACE",
            "timeout 60 cargo run 2>&1 | grep WARN",
        ]);
        let m = mine(&it, 3).0;
        assert_eq!(m.len(), 1);
        assert!(
            m[0].body.contains("2>&1"),
            "redirect mangled: {}",
            m[0].body
        );
        assert!(!m[0].body.contains("2 >& 1"));
    }
}
