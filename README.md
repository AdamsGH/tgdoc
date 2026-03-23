# tgdoc

Rust CLI that converts Telegram ecosystem documentation into structured,
Obsidian-compatible Markdown. Every internal cross-reference becomes a
`[[wiki-link]]` pointing to the correct file and heading.

## Sources

Configured in `sources.toml`. Currently two sources:

| id | driver | parser | What it produces |
|---|---|---|---|
| `tg-bot-api` | `http` | `tg-html` | Scrapes `core.telegram.org` - Bot API reference, changelog, guides |
| `ptb` | `git` | `ptb` | Clones `python-telegram-bot` repo - class/method docs, changelog |

## Output structure

```
docs/
  tg-bot-api/
    api/
      index.md
      available-types.md
      available-methods.md
      getting-updates.md
      ...
    changelog/
      index.md
      2026/
        index.md
        BotAPI-9.5.md
      2025/
        ...
    webapps/
    payments-guide/
    bots.md
    faq.md
    inline.md
    webhooks.md
    ...

  ptb/
    index.md
    telegram/
      Bot.md
      Message.md
      Chat.md
      ...
    telegram/ext/
      Application.md
      CommandHandler.md
      Filters.md
      ...
    telegram/request/
      BaseRequest.md
      HTTPXRequest.md
    changelog/
      index.md
      2026/
        v22.7.md
        v22.6.md
      2025/
        ...
      2017/
        ...
```

Each file has YAML frontmatter with `title`, `source`, `tags`, and (for
changelog entries) `date`.

## Usage

```sh
# Fetch all sources
just fetch

# Fetch a single source
just fetch tg-bot-api
just fetch ptb

# Dry-run - print structure without writing files
just dry
just dry ptb

# Clean and re-fetch
just refetch

# Pack docs/ into a timestamped tar.gz
just pack

# Fetch then pack
just all

# Remove docs/ and archives
just clean
```

Or directly via cargo:

```sh
cargo run --release -- fetch --config sources.toml --out docs
cargo run --release -- fetch tg-bot-api --dry
```

## sources.toml

```toml
[[source]]
id     = "tg-bot-api"
driver = "http"
parser = "tg-html"
out    = "tg-bot-api"

[source.http]
base_url = "https://core.telegram.org"
proxy    = "http://127.0.0.1:8580"   # optional

[[source]]
id     = "ptb"
driver = "git"
parser = "ptb"
out    = "ptb"

[source.git]
repo = "https://github.com/python-telegram-bot/python-telegram-bot"
ref  = "master"
```

Adding a new source requires: a `sources.toml` entry, a `src/source/<name>.rs`
file implementing `run(cfg, raw, out_dir, dry)`, and a line in
`src/source/mod.rs`.

## How links work

**tg-bot-api:** two-pass HTML scrape. First pass builds an anchor index
(`#sendMessage` -> `[[tg-bot-api/api/available-methods#sendMessage]]`).
Second pass converts every `<a href>` through the index.

**ptb:** anchor index is built from parsed class and method names
(`bot.send_message` -> `[[ptb/telegram/Bot#send_message]]`). RST `:class:`
and `:meth:` directives in docstrings are resolved through the same index,
producing cross-links between sources where possible.

## Drivers

| driver | behaviour |
|---|---|
| `http` | `reqwest` + optional proxy, gzip. Fetches URLs defined by the parser. |
| `git` | `git clone --depth 1` on first run, `git pull` on subsequent runs. Clone stored in `repos/<id>/` (gitignored). |

## Releases

```sh
# Tag and push - triggers GitHub Actions to build x86_64 + aarch64 binaries
just tag-release          # tags v<version in Cargo.toml>
just tag-release 1.1.0    # bumps Cargo.toml, commits, then tags

# Re-trigger an existing tag
just retag
```

## Installation

Download a release archive from the [Releases](https://github.com/AdamsGH/tgdoc/releases)
page. Each archive contains:

```
tgdoc           # binary
sources.toml    # source configuration
justfile        # convenience recipes
README.md
```

Extract and run from the same directory:

```sh
tar -xzf tgdoc-x86_64-unknown-linux-gnu.tar.gz
./tgdoc fetch
```

`tgdoc` looks for `sources.toml` in this order:

1. Path passed via `--config <path>`
2. `sources.toml` in the current working directory
3. `sources.toml` next to the binary

So running from the extracted directory works out of the box. If you move the
binary to `$PATH`, either keep `sources.toml` in your working directory or
always pass `--config /path/to/sources.toml`.

## Notes

- `docs/`, `repos/`, and `*.tar.gz` are gitignored
- `tg-bot-api` requires a proxy to reach `core.telegram.org`
- `ptb` clones from GitHub directly (no proxy needed)
