<div align="center">

# histmine

**Turn your shell history into reusable fish functions**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![CI](https://github.com/HudsonGraeme/histmine/actions/workflows/ci.yml/badge.svg)](https://github.com/HudsonGraeme/histmine/actions/workflows/ci.yml)

</div>

---

## What is this?

A zero-dependency Rust CLI that mines your shell history for commands you keep retyping with small variations, then synthesizes parameterized [fish](https://fishshell.com) functions from them.

Unlike alias generators that just rank your most frequent commands verbatim, histmine uses **anti-unification** (least general generalization): it aligns similar commands, holds the constant parts fixed, and turns the parts that vary into arguments. Slots that always change together collapse into a single parameter, and it even generalizes inside quoted sub-commands.

It reads `~/.local/share/fish/fish_history` by default, and also plain bash/zsh history files. Functions are written to stdout, a human-readable report to stderr, and anything whose constant part looks like a credential is withheld.

---

## Example

Given history like this:

```
ssh prod-3 "journalctl -u api --since '2 hours ago'"
ssh prod-7 "journalctl -u worker --since '10 min ago'"
ssh prod-1 "journalctl -u api --since '1 hour ago'"
```

histmine generalizes the three varying slots, recursing into the quoted command:

```fish
function ssh_journalctl
    ssh $argv[1] "journalctl -u $argv[2] --since '$argv[3]'"
end
```

Correlated slots share one argument. These three:

```
docker tag app:v1 && docker push app:v1
docker tag app:v2 && docker push app:v2
docker tag app:v3 && docker push app:v3
```

become a one-argument function, because the tag and the push target are always equal:

```fish
function docker_tag_push
    docker tag $argv[1] && docker push $argv[1]
end
```

---

## Usage

```bash
# Mine the default fish history, print the top 25 functions
histmine

# Try them in the current session
histmine | source

# Keep one permanently
histmine > ~/.config/fish/functions/myfunc.fish

# Mine a specific history file, tune thresholds
histmine ~/.bash_history --min 5 --top 0
```

| Flag      | Default | Meaning                                                        |
|-----------|---------|---------------------------------------------------------------|
| `--min N` | `3`     | Require at least N matching history entries to synthesize      |
| `--top N` | `25`    | Emit only the N highest-value functions (`0` = all)           |

Value is ranked by estimated keystrokes saved (entries matched against length reduction).

---

## Install

```bash
cargo install --git https://github.com/HudsonGraeme/histmine
```

Or grab a prebuilt binary for your platform from [Releases](https://github.com/HudsonGraeme/histmine/releases/latest).

---

## Build from source

```bash
git clone https://github.com/HudsonGraeme/histmine
cd histmine
cargo build --release
# binary at target/release/histmine
```

No dependencies beyond the standard library.

---

## License

MIT
