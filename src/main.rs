use std::env;
use std::path::PathBuf;
use std::process::exit;

mod history;
mod mine;
mod tokenize;

fn main() {
    let mut path: Option<PathBuf> = None;
    let mut min = 3usize;
    let mut top = 25usize;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--min" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => min = n,
                None => {
                    eprintln!("histmine: --min requires a number");
                    exit(2);
                }
            },
            "--top" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => top = n,
                None => {
                    eprintln!("histmine: --top requires a number");
                    exit(2);
                }
            },
            "-h" | "--help" => {
                eprintln!("usage: histmine [history-file] [--min N] [--top N]");
                eprintln!("  mines shell history for repeated command templates and");
                eprintln!("  emits fish functions to stdout (report on stderr)");
                eprintln!("  --min N   require at least N matching history entries (default 3)");
                eprintln!("  --top N   emit only the N highest-value functions, 0 = all (default 25)");
                eprintln!("  default file: ~/.local/share/fish/fish_history");
                eprintln!("  also reads plain bash/zsh history files");
                exit(0);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }
    let path = path.unwrap_or_else(|| {
        PathBuf::from(env::var("HOME").unwrap_or_default())
            .join(".local/share/fish/fish_history")
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
        if let Some(toks) = tokenize::tokenize(&cmd) {
            if toks.len() >= 3 {
                items.push((cmd, toks));
            }
        }
    }
    let (mut mined, withheld) = mine::mine(&items, min);
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
        eprint!(" ({} more suppressed, --top 0 for all)", found - mined.len());
    }
    eprintln!();
    if withheld > 0 {
        eprintln!(
            "{} templates withheld: constant part looks like a credential",
            withheld
        );
    }
    if !mined.is_empty() {
        eprintln!();
        for m in &mined {
            eprintln!(
                "  {:<28} {:>4} uses  {:>2} args  ~{} keystrokes",
                m.name, m.n_entries, m.n_params, m.saved
            );
        }
        eprintln!("\npipe stdout to `source` to try them, or save under ~/.config/fish/functions/");
    }
    for m in &mined {
        println!(
            "# synthesized from {} history entries (~{} keystrokes saved)",
            m.n_entries, m.saved
        );
        println!("# from: {}", m.from_example);
        println!("# try:  {}", m.try_example);
        println!("function {}", m.name);
        println!("    {}", m.body);
        println!("end");
        println!();
    }
}
