use std::env;
use std::path::PathBuf;
use std::process::exit;

mod history;
mod mine;
mod suggest;
mod tokenize;

use mine::Mined;
use suggest::Shell;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("match") => suggest::run_match(&args[1..]),
        Some("install") => run_install(&args[1..]),
        Some("uninstall") => suggest::uninstall(parse_opts(&args[1..]).shell),
        _ if args
            .iter()
            .any(|a| a == "--install-hook" || a == "--print-hook") =>
        {
            run_hook(&args)
        }
        _ => run_mine(&args),
    }
}

struct Opts {
    path: PathBuf,
    min: usize,
    top: usize,
    shell: Shell,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut path: Option<PathBuf> = None;
    let mut min = 3usize;
    let mut top = 25usize;
    let mut shell = suggest::detect_shell();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--min" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => min = n,
                None => fail("--min requires a number"),
            },
            "--top" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => top = n,
                None => fail("--top requires a number"),
            },
            "--shell" => match it.next().and_then(|v| Shell::parse(v)) {
                Some(s) => shell = s,
                None => fail("--shell requires one of: fish, bash, zsh"),
            },
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
    Opts {
        path,
        min,
        top,
        shell,
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("histmine: {msg}");
    exit(2);
}

struct Outcome {
    total: usize,
    mineable: usize,
    found: usize,
    withheld: usize,
    mined: Vec<Mined>,
}

// Read history, mine it, and truncate to the top-N. Shared by mine and install.
fn load(opts: &Opts) -> Outcome {
    let raw = match history::parse(&opts.path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("histmine: cannot read {}: {}", opts.path.display(), e);
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
    let mineable = items.len();
    let (mut mined, withheld) = mine::mine(&items, opts.min);
    let found = mined.len();
    if opts.top > 0 && mined.len() > opts.top {
        mined.truncate(opts.top);
    }
    Outcome {
        total,
        mineable,
        found,
        withheld,
        mined,
    }
}

fn report(o: &Outcome) {
    eprintln!(
        "histmine: {} history entries, {} mineable (single-line, 3+ tokens)",
        o.total, o.mineable
    );
    let saved: i64 = o.mined.iter().map(|m| m.saved).sum();
    eprint!(
        "{} functions synthesized, ~{saved} redundant keystrokes recoverable",
        o.mined.len()
    );
    if o.found > o.mined.len() {
        eprint!(
            " ({} more suppressed, --top 0 for all)",
            o.found - o.mined.len()
        );
    }
    eprintln!();
    if o.withheld > 0 {
        eprintln!(
            "{} templates withheld: constant part looks like a credential",
            o.withheld
        );
    }
    if !o.mined.is_empty() {
        eprintln!();
        for m in &o.mined {
            eprintln!(
                "  {:<28} {:>4} uses  {:>2} args  ~{} keystrokes",
                m.name, m.n_entries, m.n_params, m.saved
            );
        }
    }
}

fn run_mine(args: &[String]) {
    let opts = parse_opts(args);
    let o = load(&opts);
    suggest::write_manifest(&o.mined);
    report(&o);
    if !o.mined.is_empty() {
        eprintln!(
            "\nemitting {} functions; `histmine install` saves them and wires up nudges for you",
            opts.shell.name()
        );
    }
    for m in &o.mined {
        println!(
            "# synthesized from {} history entries (~{} keystrokes saved)",
            m.n_entries, m.saved
        );
        println!("# from: {}", m.from_example);
        println!("# try:  {}", m.try_example);
        println!("{}", suggest::render_function(&m.name, &m.body, opts.shell));
        println!();
    }
}

fn run_install(args: &[String]) {
    let opts = parse_opts(args);
    let o = load(&opts);
    report(&o);
    if o.mined.is_empty() {
        eprintln!("\nhistmine: nothing repeated enough to install (try --min 2)");
        return;
    }
    suggest::write_manifest(&o.mined);
    match suggest::install_functions(&o.mined, opts.shell) {
        Ok((written, skipped)) => {
            eprintln!(
                "\nhistmine: saved {written} {} functions",
                opts.shell.name()
            );
            if !skipped.is_empty() {
                eprintln!(
                    "skipped {} name(s) that already exist and are not histmine-managed: {}",
                    skipped.len(),
                    skipped.join(", ")
                );
            }
        }
        Err(e) => {
            eprintln!("histmine: could not save functions: {e}");
            exit(1);
        }
    }
    suggest::install_hook(opts.shell, false);
    eprintln!("\nDone. Open a new shell, then run a command you repeat: histmine will nudge you.");
    eprintln!("Undo everything with `histmine uninstall`.");
}

fn run_hook(args: &[String]) {
    let opts = parse_opts(args);
    let print_only = args.iter().any(|a| a == "--print-hook");
    suggest::install_hook(opts.shell, print_only);
}

fn help() {
    eprintln!("usage:");
    eprintln!("  histmine install [history-file] [--min N] [--top N] [--shell fish|bash|zsh]");
    eprintln!("      one-shot setup: mine history, save the functions where your shell loads");
    eprintln!("      them, and install the nudge hook. The recommended way to start.");
    eprintln!("  histmine uninstall [--shell ...]");
    eprintln!("      remove the hook, every histmine-managed function, and the manifest");
    eprintln!("  histmine [history-file] [--min N] [--top N] [--shell ...]");
    eprintln!("      mine and print functions to stdout without installing anything");
    eprintln!("  histmine match [--field name] [--] <command>");
    eprintln!("      if <command> is an instance of a mined function, suggest the short form");
    eprintln!("  histmine --install-hook | --print-hook [--shell ...]");
    eprintln!("      install (or just print) the nudge hook on its own");
    eprintln!();
    eprintln!("  --min N   require at least N matching history entries (default 3)");
    eprintln!("  --top N   keep only the N highest-value functions, 0 = all (default 25)");
    eprintln!("  --shell   target shell (default: detected from $SHELL)");
    eprintln!("  default history file: ~/.local/share/fish/fish_history (also bash/zsh files)");
}
