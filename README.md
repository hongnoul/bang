# bang

Search from the terminal with engine-native `!bangs`.

```sh
bang '!w rust lifetimes'    # -> prints the Wikipedia search URL
bang '!gh ripgrep'          # -> prints the GitHub search URL
bang rust lifetimes         # -> ranked results in your terminal
bang -o '!yt lofi'          # -> opens YouTube in your browser
bang --json linux wayland   # -> JSON for fzf/rofi/scripting pipelines
```

Queries pass through to the engine **verbatim**. Bangs are resolved
server-side (DuckDuckGo's ~13k bangs, or your SearXNG instance's engine
shortcuts), so there is nothing to configure, no bang list to update, and
your browser muscle memory just works.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/hongnoul/bang/master/install.sh | sh
```

Update any time with `bang update`. Installs are versioned under
`~/.local/share/bang/versions/` with an atomic `current` symlink, so
updates never corrupt a running binary and rollback is one `ln -sfn`.

## Configuration

One optional file, `~/.config/bang/config.toml`:

```toml
engine = "duckduckgo"                    # or "searxng"
searxng_url = "https://searx.example.org"
```

Env overrides: `BANG_ENGINE`, `BANG_SEARXNG_URL`.

With SearXNG, engine bangs (`!go`, `!bi`, `!wp`, ...) select the upstream
engine server-side and results still come back as structured JSON, so you
get native in-terminal results for every engine your instance supports.

## No alias config, on purpose

The bang vocabulary belongs to the engine and shorthand belongs to your
shell. If you want custom shortcuts:

```sh
alias ghs="bang '!gh'"
wiki() { bang "!w $*"; }
```

Custom bangs belong in your SearXNG instance settings, where they are
visible to every client, not just this one.

## Tiling WM integration

`bang` binds cleanly because it is just a CLI. Examples:

**sway / i3** (`$mod+s` opens a floating search scratch terminal):

```
bindsym $mod+s exec foot --app-id=bang-search -e sh -c 'read -r -p "search: " q && bang -o "$q"'
for_window [app_id="bang-search"] floating enable
```

**hyprland:**

```
bind = $mod, S, exec, foot --app-id=bang-search -e sh -c 'read -r -p "search: " q && bang -o "$q"'
windowrulev2 = float, class:(bang-search)
```

**rofi / fuzzel** (no terminal at all):

```sh
q=$(fuzzel --dmenu --prompt='search: ') && bang -o "$q"
```

We ship no default keybinds and never edit your WM config. The stable
surface is the `bang` CLI and the `bang-search` app-id convention above.

## Releases

Tag-triggered CI builds static binaries for Linux (x86_64, aarch64) and
macOS (x86_64, aarch64) with SHA-256 checksums. The self-updater verifies
checksums and authenticates GitHub API requests via `GH_TOKEN`/`GITHUB_TOKEN`
or `gh auth token` when available, so update checks don't fall into the
shared unauthenticated per-IP rate limit bucket.

## License

MIT
