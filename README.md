# fdu — Fast Disk Usage

A fast, parallel disk usage analyzer written in Rust. Think `du`, but multithreaded.

## Features

- **Parallel filesystem walking** using crossbeam work-stealing queues
- **Multiple output formats** — human-readable, SI, bytes, hex, JSON, raw
- **Flexible filtering** — include/exclude patterns via regex, depth limits, size thresholds
- **Symlink & hardlink handling** — with configurable caching
- **Sorting** — by name, size, count, or modification time
- **Cross-platform** — standard Rust filesystem APIs

## Usage

```bash
# Analyze current directory
fdu

# Analyze specific paths with human-readable sizes
fdu /home /var -F human

# Show only directories, max depth 2
fdu -d -L 2 /home

# Sort by size, reversed, JSON output
fdu -S size -r -o json /home

# Exclude patterns, show only files over 100MB
fdu -f --exclude "node_modules" --exclude ".git" -t 100M /home

# Use 16 threads
fdu -j 16 /home
```

## Options

```
USAGE: fdu [OPTIONS] [PATH]...

ARGUMENTS:
  [PATH]...          Paths to analyze [default: .]

OPTIONS:
  -a, --all                   Display all files and directories
  -d, --dirs-only             Display only directories
  -f, --files-only            Display only files
  -F, --format <FORMAT>       Size format: human, si, blocks, bytes, binary, hex, kilo, mega, giga
  -L, --max-depth <N>         Maximum depth
      --min-depth <N>         Minimum depth
  -s, --summarize             Display only a total for each path
  -S, --sort <FIELD>          Sort by: name, size, count, time
  -r, --reverse               Reverse sort order
  -c, --total                 Produce grand total
  -t, --threshold <SIZE>      Minimum size threshold
      --include <PATTERN>     Include only matching paths (regex)
      --exclude <PATTERN>     Exclude matching paths (regex)
  -j, --jobs <N>              Number of threads [default: 32]
  -o, --output <FORMAT>       Output format: raw, json
  -H, --dereference           Follow symlinks
  -x, --one-file-system       Don't cross filesystem boundaries
  -l, --count-links           Count hard links
      --apparent-size         Display apparent size instead of disk usage
      --time                  Show modification time
  -h, --help                  Print help
  -V, --version               Print version
```

## Building

```bash
git clone https://github.com/Mediacom99/fdu.git
cd fdu
cargo build --release
```

The release binary is at `target/release/fdu`.

## Architecture

- **Walker** — parallel filesystem traversal using crossbeam work-stealing deques and channels
- **Processor** — reconstructs the directory tree from the walker's output using concurrent hash maps
- **CLI** — clap-derive based argument parsing with rich option support

## Dependencies

- [crossbeam](https://github.com/crossbeam-rs/crossbeam) — lock-free channels and work-stealing deques
- [clap](https://github.com/clap-rs/clap) — CLI argument parsing
- [humansize](https://github.com/LeopoldArkworx/humansize) — human-readable size formatting
- [regex](https://github.com/rust-lang/regex) — pattern matching for include/exclude filters
- [fastrace](https://github.com/fastracelabs/fastrace) — optional tracing instrumentation

## License

MIT OR Apache-2.0
