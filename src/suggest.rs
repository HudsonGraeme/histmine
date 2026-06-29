use crate::mine::{Mined, quoted_arg};
use crate::tokenize::{Quote, Token, tokenize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Fish,
    Bash,
    Zsh,
}

impl Shell {
    pub fn parse(s: &str) -> Option<Shell> {
        match s {
            "fish" => Some(Shell::Fish),
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Shell::Fish => "fish",
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
        }
    }
}

pub fn detect_shell() -> Shell {
    let sh = env::var("SHELL").unwrap_or_default();
    let base = sh.rsplit('/').next().unwrap_or("");
    if base.contains("zsh") {
        Shell::Zsh
    } else if base.contains("bash") {
        Shell::Bash
    } else {
        Shell::Fish
    }
}

fn home() -> PathBuf {
    PathBuf::from(env::var("HOME").unwrap_or_default())
}

fn config_dir() -> PathBuf {
    match env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(".config"),
    }
}

fn data_dir() -> PathBuf {
    match env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(".local/share"),
    }
}

pub fn manifest_path() -> PathBuf {
    data_dir().join("histmine/manifest.tsv")
}

// One line per template: `name<TAB>fish-body`. The rendered fish body carries the
// full structure (consts plus `$argv[N]` slots), so it is a complete and zero-dep
// serialization that match-mode reads back to reconstruct the template.
pub fn write_manifest(mined: &[Mined]) {
    let path = manifest_path();
    let mut out = String::new();
    for m in mined {
        if m.name.contains('\t') || m.body.contains('\t') || m.body.contains('\n') {
            continue;
        }
        out.push_str(&m.name);
        out.push('\t');
        out.push_str(&m.body);
        out.push('\n');
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = fs::write(&path, out) {
        eprintln!("histmine: could not write manifest {}: {e}", path.display());
        return;
    }
    eprintln!(
        "histmine: manifest updated ({} templates) at {}",
        mined.len(),
        path.display()
    );
}

fn read_manifest() -> Vec<(String, String)> {
    let data = match fs::read_to_string(manifest_path()) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    data.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(n, b)| (n.to_string(), b.to_string()))
        .collect()
}

fn argv_index(value: &str) -> Option<usize> {
    value
        .strip_prefix("$argv[")
        .and_then(|r| r.strip_suffix(']'))
        .and_then(|d| d.parse::<usize>().ok())
}

enum MatchPiece {
    Const(String),
    Param(usize),
    Inner(Vec<InnerMatch>),
}

enum InnerMatch {
    Const(String),
    Param(usize),
}

fn parse_template(body: &str) -> Option<Vec<MatchPiece>> {
    let toks = tokenize(body)?;
    let mut pieces = Vec::new();
    for t in &toks {
        if !t.op && argv_index(&t.value).is_some() {
            pieces.push(MatchPiece::Param(argv_index(&t.value).unwrap()));
        } else if t.quote == Quote::Double && t.value.contains("$argv[") {
            let inner_toks = tokenize(&t.value)?;
            let mut inner = Vec::new();
            for it in &inner_toks {
                match argv_index(&it.value) {
                    Some(n) => inner.push(InnerMatch::Param(n)),
                    None => inner.push(InnerMatch::Const(it.raw.clone())),
                }
            }
            pieces.push(MatchPiece::Inner(inner));
        } else {
            pieces.push(MatchPiece::Const(t.raw.clone()));
        }
    }
    Some(pieces)
}

fn capture(caps: &mut BTreeMap<usize, String>, n: usize, value: &str) -> bool {
    match caps.get(&n) {
        Some(prev) => prev == value,
        None => {
            caps.insert(n, value.to_string());
            true
        }
    }
}

fn try_match(pieces: &[MatchPiece], input: &[Token]) -> Option<BTreeMap<usize, String>> {
    if pieces.len() != input.len() {
        return None;
    }
    let mut caps = BTreeMap::new();
    for (p, tok) in pieces.iter().zip(input) {
        match p {
            MatchPiece::Const(raw) => {
                if &tok.raw != raw {
                    return None;
                }
            }
            MatchPiece::Param(n) => {
                if !capture(&mut caps, *n, &tok.value) {
                    return None;
                }
            }
            MatchPiece::Inner(inner) => {
                if tok.quote != Quote::Double {
                    return None;
                }
                let inner_toks = tokenize(&tok.value)?;
                if inner_toks.len() != inner.len() {
                    return None;
                }
                for (ip, it) in inner.iter().zip(&inner_toks) {
                    match ip {
                        InnerMatch::Const(raw) => {
                            if &it.raw != raw {
                                return None;
                            }
                        }
                        InnerMatch::Param(n) => {
                            if !capture(&mut caps, *n, &it.value) {
                                return None;
                            }
                        }
                    }
                }
            }
        }
    }
    Some(caps)
}

fn const_count(pieces: &[MatchPiece]) -> usize {
    pieces
        .iter()
        .filter(|p| matches!(p, MatchPiece::Const(_)))
        .count()
}

// Returns (function name, rendered argument string) for the most specific template
// the given command is an instance of.
fn best_match(cmd: &str) -> Option<(String, String)> {
    let input = tokenize(cmd)?;
    let head = &input[0].raw;
    let mut best: Option<(usize, String, String)> = None;
    for (name, body) in read_manifest() {
        let pieces = match parse_template(&body) {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };
        match &pieces[0] {
            MatchPiece::Const(h) if h == head => {}
            _ => continue,
        }
        if let Some(caps) = try_match(&pieces, &input) {
            let args = caps
                .values()
                .map(|v| quoted_arg(v))
                .collect::<Vec<_>>()
                .join(" ");
            let score = const_count(&pieces);
            if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
                best = Some((score, name, args));
            }
        }
    }
    best.map(|(_, name, args)| (name, args))
}

// `histmine match [--field name] [--] <command...>`
pub fn run_match(rest: &[String]) {
    let mut field_name = false;
    let mut parts: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--field" => {
                field_name = rest.get(i + 1).map(|s| s == "name").unwrap_or(false);
                i += 2;
            }
            "--" => {
                parts.extend(rest[i + 1..].iter().map(|s| s.as_str()));
                break;
            }
            other => {
                parts.push(other);
                i += 1;
            }
        }
    }
    let cmd = parts.join(" ");
    if cmd.trim().is_empty() {
        return;
    }
    if let Some((name, args)) = best_match(&cmd) {
        if field_name {
            println!("{name}");
        } else {
            println!("histmine: simplified to `{name}` -> try: {name} {args}");
        }
    }
}

// Translate a fish body (`$argv[N]` slots) into a POSIX body (`$N`). Bare slots are
// quoted to stop word-splitting; slots inside a double-quoted blob need no extra
// quoting because the surrounding double quotes already protect them.
fn to_posix_body(fish_body: &str) -> String {
    let chars: Vec<char> = fish_body.chars().collect();
    let mut out = String::new();
    let mut in_dquote = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '"' {
            in_dquote = !in_dquote;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '$' && chars[i..].starts_with(&['$', 'a', 'r', 'g', 'v', '[']) {
            let mut j = i + 6;
            let mut num = String::new();
            while j < chars.len() && chars[j].is_ascii_digit() {
                num.push(chars[j]);
                j += 1;
            }
            if j < chars.len() && chars[j] == ']' && !num.is_empty() {
                if in_dquote {
                    out.push('$');
                    out.push_str(&num);
                } else {
                    out.push_str(&format!("\"${num}\""));
                }
                i = j + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

pub fn render_function(name: &str, fish_body: &str, shell: Shell) -> String {
    match shell {
        Shell::Fish => format!("function {name}\n    {fish_body}\nend"),
        Shell::Bash | Shell::Zsh => {
            format!("{name}() {{\n    {}\n}}", to_posix_body(fish_body))
        }
    }
}

const MARKER_OPEN: &str = "# >>> histmine hook >>>";
const MARKER_CLOSE: &str = "# <<< histmine hook <<<";

fn hook_snippet(shell: Shell) -> String {
    let body = match shell {
        Shell::Fish => {
            "function __histmine_nudge --on-event fish_preexec\n    \
             set -l name (command histmine match --field name -- $argv 2>/dev/null)\n    \
             test -n \"$name\"; and functions -q -- $name; and command histmine match -- $argv 2>/dev/null\n\
             end"
        }
        Shell::Zsh => {
            "__histmine_nudge() {\n    emulate -L zsh\n    local name\n    \
             name=$(command histmine match --field name -- \"$1\" 2>/dev/null) || return\n    \
             [[ -n $name ]] && whence -w -- \"$name\" >/dev/null 2>&1 && command histmine match -- \"$1\" 2>/dev/null\n}\n\
             autoload -Uz add-zsh-hook 2>/dev/null && add-zsh-hook preexec __histmine_nudge"
        }
        Shell::Bash => {
            // bash has no native full-line preexec (its DEBUG trap fires per
            // sub-command, breaking compound lines), so read the whole last line from
            // history at the prompt. The nudge therefore appears just after the command.
            "__histmine_last=\"\"\n\
             __histmine_nudge() {\n    local cmd name\n    \
             cmd=$(HISTTIMEFORMAT= history 1)\n    \
             [[ $cmd =~ ^[[:space:]]*[0-9]+[[:space:]]+(.*)$ ]] && cmd=\"${BASH_REMATCH[1]}\" || return\n    \
             [[ $cmd == \"$__histmine_last\" ]] && return\n    \
             __histmine_last=$cmd\n    \
             case \"$cmd\" in *\"histmine match\"*|__histmine_*) return;; esac\n    \
             name=$(command histmine match --field name -- \"$cmd\" 2>/dev/null)\n    \
             [[ -n $name ]] && type -t -- \"$name\" >/dev/null 2>&1 && command histmine match -- \"$cmd\" 2>/dev/null\n}\n\
             case \"$PROMPT_COMMAND\" in *__histmine_nudge*) ;; *) PROMPT_COMMAND=\"__histmine_nudge${PROMPT_COMMAND:+; $PROMPT_COMMAND}\";; esac"
        }
    };
    format!(
        "{MARKER_OPEN}\n# Suggests a mined function when you run a command it has generalized,\n# only if that function is currently defined. Remove this block to disable.\n{body}\n{MARKER_CLOSE}\n"
    )
}

fn rc_path(shell: Shell) -> PathBuf {
    match shell {
        Shell::Fish => config_dir().join("fish/conf.d/histmine.fish"),
        Shell::Bash => home().join(".bashrc"),
        Shell::Zsh => home().join(".zshrc"),
    }
}

pub fn install_hook(shell: Shell, print_only: bool) {
    let snippet = hook_snippet(shell);
    if print_only {
        print!("{snippet}");
        return;
    }
    let path = rc_path(shell);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.contains(MARKER_OPEN) {
        eprintln!(
            "histmine: hook already installed in {} (nothing to do)",
            path.display()
        );
        return;
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("histmine: cannot create {}: {e}", parent.display());
            return;
        }
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&snippet);
    if let Err(e) = fs::write(&path, content) {
        eprintln!("histmine: cannot write {}: {e}", path.display());
        return;
    }
    eprintln!(
        "histmine: installed {} hook in {} (restart your shell or re-source it)",
        shell.name(),
        path.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_simple_template_and_renders_args() {
        let (name, args) = best_template(
            "git_push",
            "git push origin $argv[1]",
            "git push origin main",
        );
        assert_eq!(name, "git_push");
        assert_eq!(args, "main");
    }

    #[test]
    fn matches_inner_and_correlated() {
        let (_, args) = best_template(
            "ssh_journalctl",
            "ssh $argv[1] \"journalctl -u $argv[2] --since '$argv[3]'\"",
            "ssh prod-3 \"journalctl -u api --since '2 hours ago'\"",
        );
        assert_eq!(args, "prod-3 api '2 hours ago'");
    }

    #[test]
    fn correlated_slot_must_be_consistent() {
        let body = "docker tag $argv[1] && docker push $argv[1]";
        let pieces = parse_template(body).unwrap();
        assert!(try_match(&pieces, &tokenize("docker tag a && docker push a").unwrap()).is_some());
        assert!(try_match(&pieces, &tokenize("docker tag a && docker push b").unwrap()).is_none());
    }

    #[test]
    fn non_matching_command_yields_nothing() {
        let pieces = parse_template("git push origin $argv[1]").unwrap();
        assert!(try_match(&pieces, &tokenize("git commit -m x").unwrap()).is_none());
    }

    #[test]
    fn posix_body_quotes_bare_slots_only() {
        let p = to_posix_body("ssh $argv[1] \"journalctl -u $argv[2] --since '$argv[3]'\"");
        assert_eq!(p, "ssh \"$1\" \"journalctl -u $2 --since '$3'\"");
    }

    #[test]
    fn posix_correlated_slot() {
        assert_eq!(
            to_posix_body("docker tag $argv[1] && docker push $argv[1]"),
            "docker tag \"$1\" && docker push \"$1\""
        );
    }

    fn best_template(name: &str, body: &str, cmd: &str) -> (String, String) {
        let pieces = parse_template(body).unwrap();
        let input = tokenize(cmd).unwrap();
        let caps = try_match(&pieces, &input).unwrap();
        let args = caps
            .values()
            .map(|v| quoted_arg(v))
            .collect::<Vec<_>>()
            .join(" ");
        (name.to_string(), args)
    }
}
