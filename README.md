# tgdoc

Rust CLI that scrapes the Telegram Bot API documentation from `core.telegram.org`
and converts it into structured, Obsidian-compatible Markdown files.

Every internal cross-reference becomes an `[[wiki-link]]` pointing to the correct
file and heading. The output is ready to open as an Obsidian vault.

## Output structure

```
docs/
  api/
    index.md
    getting-updates.md
    available-types.md
    available-methods.md
    updating-messages.md
    inline-mode.md
    payments.md
    stickers.md
    telegram-passport.md
    games.md
    ...
  changelog/
    index.md
    2026/
      index.md
      BotAPI-9.5.md      # date: 2026-03-01 in frontmatter
      BotAPI-9.4.md
    2025/
      BotAPI-9.3.md
      ...
  webapps/
    index.md
    designing-mini-apps.md
    implementing-mini-apps.md
    initializing-mini-apps.md
    testing-mini-apps.md
  payments-guide/
    index.md
    the-payments-api.md
    step-by-step-process.md
    going-live.md
    faq.md
  bots.md
  faq.md
  inline.md
  webhooks.md
  self-signed.md
  stickers.md
  passport.md
  widgets-login.md
```

Each file has a YAML frontmatter block with `title`, `source`, `tags`, and
(for changelog entries) `date`.

## Usage

```sh
# Fetch all pages and write docs/
just fetch

# Dry-run - print heading tree without writing anything
just dry

# Clean docs/ and re-fetch
just refetch

# Pack docs/ into a timestamped tar.gz
just pack

# Fetch then pack in one step
just all

# Remove docs/ and archives
just clean
```

The proxy is configured at the top of `justfile`:

```
proxy := "http://127.0.0.1:8580"
```

Or pass it directly:

```sh
cargo run --release -- fetch --proxy http://127.0.0.1:8580 --out docs
```

## Pages fetched

| Source URL | Output |
|---|---|
| `/bots/api` | `api/*.md` - split by section |
| `/bots/api-changelog` | `changelog/<year>/BotAPI-X.Y.md` - one file per release |
| `/bots/webapps` | `webapps/*.md` - split by section |
| `/bots/payments` | `payments-guide/*.md` - split by section |
| `/bots` | `bots.md` |
| `/bots/faq` | `faq.md` |
| `/bots/inline` | `inline.md` |
| `/bots/webhooks` | `webhooks.md` |
| `/bots/self-signed` | `self-signed.md` |
| `/stickers` | `stickers.md` |
| `/passport` | `passport.md` |
| `/widgets/login` | `widgets-login.md` |

## How links work

The parser builds a global anchor index in a first pass over all pages.
In the second pass every `<a href="...#anchor">` is resolved through the index
and written as `[[file#Heading]]`. Links that cannot be resolved are kept as
plain Markdown URLs.

## Dependencies

- `scraper` - HTML parsing
- `reqwest` - HTTP with gzip and proxy support
- `tokio` - async runtime
- `clap` - CLI
- `regex` - version extraction from changelog text

## Notes

- `docs/` and `*.tar.gz` are excluded from git (see `.gitignore`)
- Requires a working HTTP proxy to reach `core.telegram.org`
