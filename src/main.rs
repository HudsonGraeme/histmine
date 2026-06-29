use std::env;
use std::path::PathBuf;
use std::process::exit;

mod history;
mod mine;
mod suggest;
mod tokenize;

use suggest::Shell;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("match") {
        suggest::run_match(&args[1..]);
        return;
    }

    if args
        .iter()
        .any(|a| a == "--install-hook" || a == "--print-hook")
    {
        run_hook(&args);
        return;
    }

    run_mine(&args);
}

fn parse_shell(args: &[String]) -> Shell {
    let mut shell = suggest::detect_shell();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--shell" {
            match it.next().and_then(|v| Shell::parse(v)) {
                Some(s) => shell = s,
                None => {
                    eprintln!("histmine: --shell requires one of: fish, bash, zsh");
                    exit(2);
                }
            }
        }
    }
    shell
}

fn run_hook(args: &[String]) {
    let print_only = args.iter().any(|a| a == "--print-hook");
    suggest::install_hook(parse_shell(args), print_only);
}

fn run_mine(args: &[String]) {
    let mut path: Option<PathBuf> = None;
    let mut min = 3usize;
    let mut top = 25usize;
    let shell = parse_shell(args);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--min" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => min = n,
                None => {
                    eprintln!("histmine: --min requires a number");
                    exit(2);
                }
            },
            "--top" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => top = n,
                None => {
                    eprintln!("histmine: --top requires a number");
                    exit(2);
                }
            },
            "--shell" => {
                it.next();
            }
            "-h" | "--help" => {
                help();
                exit(0);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }
    let path = path.unwrap_or_else(|| {
        PathBuf::from(env::var("HOME").unwrap_or_default()).join(".local/share/fish/fish_history")
    });
    let raw = match history::parse(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("histmine: cannot read {}: {}", path.display(), e);
            exit(1);
        }
    };
    let total = raw.len();
    let mut items = Vec::new();
    for cmd in raw {
        if cmd.contains('\n') {
            continue;
        }
        if let Some(toks) = tokenize::tokenize(&cmd)
            && toks.len() >= 3
        {
            items.push((cmd, toks));
        }
    }
    let (mut mined, withheld) = mine::mine(&items, min);
    suggest::write_manifest(&mined);
    let found = mined.len();
    if top > 0 && mined.len() > top {
        mined.truncate(top);
    }
    let total_saved: i64 = mined.iter().map(|m| m.saved).sum();
    eprintln!(
        "histmine: {} history entries, {} mineable (single-line, 3+ tokens)",
        total,
        items.len()
    );
    eprint!(
        "{} functions synthesized, ~{} redundant keystrokes recoverable",
        mined.len(),
        total_saved
    );
    if found > mined.len() {
        eprint!(
            " ({} more suppressed, --top 0 for all)",
            found - mined.len()
        );
    }
    eprintln!();
    if withheld > 0 {
        eprintln!("{withheld} templates withheld: constant part looks like a credential");
    }
    if !mined.is_empty() {
        eprintln!();
        for m in &mined {
            eprintln!(
                "  {:<28} {:>4} uses  {:>2} args  ~{} keystrokes",
                m.name, m.n_entries, m.n_params, m.saved
            );
        }
        eprintln!(
            "\nemitting {} functions; pipe stdout to `source`, or save where {} loads them",
            shell.name(),
            shell.name()
        );
        eprintln!("run `histmine --install-hook` to get nudged toward these as you type");
    }
    for m in &mined {
        println!(
            "# synthesized from {} history entries (~{} keystrokes saved)",
            m.n_entries, m.saved
        );
        println!("# from: {}", m.from_example);
        println!("# try:  {}", m.try_example);
        println!("{}", suggest::render_function(&m.name, &m.body, shell));
        println!();
    }
}

fn help() {
    eprintln!("usage:");
    eprintln!("  histmine [history-file] [--min N] [--top N] [--shell fish|bash|zsh]");
    eprintln!("      mine shell history for repeated command templates and emit shell");
    eprintln!("      functions to stdout (report on stderr); also writes a match manifest");
    eprintln!("  histmine match [--field name] [--] <command>");
    eprintln!("      if <command> is an instance of a mined function, suggest the short form");
    eprintln!("  histmine --install-hook [--shell fish|bash|zsh]");
    eprintln!("      install a shell preexec hook that nudges you toward mined functions");
    eprintln!("  histmine --print-hook [--shell fish|bash|zsh]");
    eprintln!("      print the hook snippet instead of installing it");
    eprintln!();
    eprintln!("  --min N   require at least N matching history entries (default 3)");
    eprintln!("  --top N   emit only the N highest-value functions, 0 = all (default 25)");
    eprintln!("  --shell   target shell syntax (default: detected from $SHELL)");
    eprintln!("  default history file: ~/.local/share/fish/fish_history");
    eprintln!("  also reads plain bash/zsh history files");
}
