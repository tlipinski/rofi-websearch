# rofi-websearch

A [rofi](https://github.com/davatorium/rofi) plugin for web searches that works
like Firefox's search keywords: `im matrix` + Enter searches IMDb for "matrix"
in the default browser (`xdg-open`).

It's a plugin rather than a dmenu script so the engine row stays visible while
the query is typed. dmenu hides rows that don't contain every typed word, and
scripts can't change the filtering.

- `kw query`: shows only that engine with the query, Enter searches it
- a single word: shows the engines whose keyword, name or site contains it;
  Enter on one (or on a row with nothing typed) fills in `kw ` for the query
- anything else: shows the default engine with the text
- `example.com/x`, `https://...`: opened as is
- Ctrl+Enter: takes the typed text as is, e.g. `gh` searches the default
  engine instead of picking GitHub
- Alt+d (rofi's `kb-custom-1`, see [Usage](#usage)): makes the highlighted
  engine the default, marked "· default". The default is always the top row,
  the others stay in the file's order. It's saved in
  `$XDG_STATE_HOME/rofi-websearch/default` (`~/.local/state/...`); without it
  the first engine in the file is the default

## Requirements

- rofi 2.0 or newer, with its headers (`/usr/include/rofi`, part of the
  `rofi` package on Arch, `rofi-dev` on Debian/Ubuntu)
- Rust (stable, edition 2024) to build
- at runtime: `sh`, `curl` for the favicons and `xdg-open` (or see
  [Browser](#browser))

## Install

```sh
make install
```

This builds the plugin and installs it as `~/.local/lib/rofi/websearch.so`.
rofi doesn't look there by default, so pass `-plugin-path ~/.local/lib/rofi`
(see below). To install into rofi's own plugin directory instead, where it's
found without the option:

```sh
make build
sudo make install PLUGIN_DIR=$(pkg-config --variable=pluginsdir rofi)
```

`make uninstall` with the same `PLUGIN_DIR` removes it.

## Usage

```sh
rofi -plugin-path ~/.local/lib/rofi -show websearch -modi websearch \
     -kb-custom-1 Alt+d -show-icons
```

Bind that to a key in your window manager. `-kb-custom-1 Alt+d` sets the key
that makes an engine the default (rofi's own default for it is Alt+1). If
your rofi config already uses Alt+d, pick another key. Other useful options:
`-display-websearch search` for the prompt text, and
`-theme-str 'element { spacing: 0.6em; }'` for some room around the icons.

| Option                       | Default                                  |
| ---------------------------- | ---------------------------------------- |
| `-websearch-engines <file>`  | `$XDG_CONFIG_HOME/rofi-websearch/engines.ini` |
| `-websearch-open <command>`  | `xdg-open`                               |

## Browser

Searches open with `xdg-open`, so in the default browser and, for Firefox,
its default profile. `-websearch-open` runs another command instead, with the
URL added as its last argument, e.g. to always use a Firefox profile (it's
reused when that profile is already running):

```sh
rofi -show websearch -modi websearch -websearch-open 'firefox -P home'
```

The command goes through `sh`, so names with spaces can be quoted.

## Engines

Read from `~/.config/rofi-websearch/engines.ini` (`$XDG_CONFIG_HOME` is
respected), or the path given with `-websearch-engines`. On the first start
that file is created from the [example engines](engines.ini) built into the
plugin, ready to edit. A missing `-websearch-engines` file isn't created; the
example engines are used instead and the error is printed to stderr.

One section per engine, named by its keyword:

```ini
[im]
name = IMDb
url  = https://www.imdb.com/find/?q={}
```

`{}` in the URL is replaced by the query, with spaces as `%20`. Changes apply
the next time rofi opens, no rebuild needed.

## Icons

Rows show the site's `/favicon.ico` (needs `-show-icons`). Missing ones are
downloaded with curl in the background into
`$XDG_CACHE_HOME/rofi-websearch/icons/` (`~/.cache/...`), named by host, and
show up the next time rofi opens. If the site's favicon can't be downloaded
(e.g. Cloudflare blocks curl), DuckDuckGo's icon service
(`icons.duckduckgo.com/ip3/<host>.ico`) is used instead. A failed download is
retried next time, delete a file there to fetch it again. An engine can use
another icon with `icon = ~/path/to/image.png` or an icon theme name like
`icon = firefox`.

Downloads use the `curl` command rather than a Rust HTTP library on purpose:
rofi usually exits a second or two after opening, which would kill download
threads inside its process, while a separate curl process finishes on its
own. It also keeps a TLS stack and its crates out of the plugin. The cost is
needing `curl` and `sh` at runtime, with failures retried silently on the
next open.

## Development

Uses the [rofi-mode](https://crates.io/crates/rofi-mode) crate for rofi's
plugin interface.

```sh
make test      # cargo test
```

`src/search.rs` has the logic (engines file, filtering, what Enter does) and
its tests, `src/rofi.rs` the glue to rofi. Rebuild after a rofi upgrade that
changes the plugin ABI (rofi-mode has to support it, `ABI_VERSION` in
`/usr/include/rofi/mode.h`, 7 for rofi 2.0).
