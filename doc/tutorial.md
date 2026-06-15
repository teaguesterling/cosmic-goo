# Tutorial: learn `goo` by example

Every block below is runnable. Lines starting with `$` are commands; the line(s) under
them are representative output (yours will reflect your own desktop). Work top to bottom —
each section builds on the last. (New to the idea? Read **[The model](the-model.md)** first.)

> Setup: install with `make install`, or from a checkout point the Rust binary at the
> repo's plugins — `export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$PWD/plugins"`. The examples
> write `goo`.

---

## 1. The sentence: verb + subject

`goo` runs a **verb** on a **subject**, where a subject is *any thing on your desktop*.
Point at a git repo and ask its status; point at an app and focus it:

```
$ goo status :repo:cosmic-goo     # `:repo:NAME` fuzzy-matches a git repo
## main...origin/main

$ goo activate :app/firefox       # focus a running app  (`:app/ID` is exact)
```

`goo what <subject>` lists the verbs a thing accepts; `goo describe <verb>` shows one
verb's details:

```
$ goo what :repo:cosmic-goo
applicable verbs for :repo:cosmic-goo  (type: application/vnd.git.repo)
    status      Show short status
    pull        Pull the current branch
    gh-pr-list  List open GitHub PRs
    open        Open in its default application   # a repo is also a directory, so it
    …                                             # inherits file verbs (open/reveal/tree…)
```

Text is just one more subject type — the same grammar gives you text verbs:

```
$ goo upper "hello world"
HELLO WORLD

$ goo sha256 "hello"
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
```

See what's loaded with `goo plugins` (~30 of them) and `goo --help` for the subcommands.

---

## 2. A tour — operate on *things*

The breadth is the point. A spread of what ships, by domain:

```
# apps & windows
$ goo activate :app/firefox            # focus an app
$ goo move-to :app:alacritty :ws/0:1   # move an app to a workspace (a two-step verb)
$ goo switch :ws/0:1                    # switch workspace

# code
$ goo status :repo:cosmic-goo           # short git status
$ goo log :br/main                      # recent commits on a branch

# files
$ goo open ./README.md                  # xdg-open
$ goo reveal ~/Pictures                 # open the containing folder
$ goo copy-path ./notes.md              # absolute path → clipboard

# devices & services  (one `connect` verb, many kinds of thing)
$ goo connect :bt:headphones            # Bluetooth
$ goo connect :ssh/prod                 # SSH host
$ goo logs :svc/woollama                # systemd unit logs

# media, screenshots, qr, notify
$ goo now-playing                       # MPRIS (no subject)
$ goo volume-up                         # default audio sink
$ goo capture-region                    # drag-select → image on the clipboard
$ goo qr-encode "https://example.com"   # a QR code, drawn in your terminal
$ goo notify "build done" --urgency=normal
```

`connect`, `status`, `logs`, `info` are **polymorphic** — one verb, dispatched to the
right implementation for whatever type you hand it. New domains are just plugins.

---

## 3. Where does the subject come from?

If you don't give a positional argument, `goo` finds a subject automatically — **stdin
(if piped) → PRIMARY selection → clipboard**. That fallback is what makes text verbs
great for keybindings: bind a key to `goo summarize` and it acts on whatever you've
highlighted.

```
$ echo "piped text" | goo upper
PIPED TEXT

$ goo summarize                # no arg → summarises your PRIMARY selection
                               # (highlight some text first, then run this)
```

---

## 4. Addressing: pointing at specific things

Every subject rewrites to one canonical `goo://<domain>/<path>` URI. The everyday rules:

```
$ goo wc ./README.md            # a FILE (./ ~/ / are read as files — contents, not the path)
$ goo open https://x.com        # a URL (scheme:// is recognized)
$ goo upper ^                    # ^ = the clipboard (built-in → goo://clip/)
$ goo activate :app:firefox      # :dom:query — fuzzy SEARCH the apps domain
$ goo switch :ws/0:1             # :dom/id — the EXACT workspace value 0:1
```

- **Files and URLs need no sigil** — `./notes.md` and `https://…` are recognized by
  shape. `+x` forces literal text (`goo upper +data.json` shouts the *string*, not the file).
- **`:dom:query` searches (fuzzy), `:dom/id` is the exact value.** Either reaches anything
  a domain lists — discover ids with `goo list`:

```
$ goo list workspaces | jq -c '.[] | {id, title}'
{"id":"0:0","title":"ws-1 on DP-3"}
{"id":"0:1","title":"ws-2 on DP-3"}
```

**Skip the verb entirely.** Give just an address and no verb and `goo` runs that type's
*default* action — the CLI form of the protocol's `GOO` verb:

```
$ goo :br/main           # no verb → `log` (the git-branch default)
$ goo ~/notes.md         # → the file default (open)
$ goo :app/firefox       # → activate
```

(If a type has no default verb, `goo` says so rather than guessing.)

---

## 5. Two-step verbs (subject + object)

A few verbs take an **object** as a second argument:

```
$ goo move-to :app:alacritty :ws/0:1   # move an app (subject) to a workspace (object)
$ goo rename :tmux/work +release        # rename a tmux session to "release" (+ forces text)
```

The object goes through the same addressing as the subject.

---

## 6. Adverbs: modifying *how* a verb runs

Adverbs are `--name=value` modifiers. The text/LLM verbs are the richest family — they
route through an adverb. By default `--via=woollama` sends the prompt to your local
[woollama](https://github.com/teaguesterling/woollama) router and prints the reply:

```
$ goo summarize "the mitochondria is the powerhouse of the cell"
Mitochondria produce the cell's ATP through respiration.
```

Route to the clipboard instead to *see* the assembled prompt (no daemon needed):

```
$ goo critique "this paragraph could be tighter" --via=clipboard
$ wl-paste | head -2
You are providing expert review of the following passage.
Deduce the desired intent and tone, then critique accordingly.
```

`--via` values: `woollama` (default — needs the daemon), `clipboard`, `claude-desktop`,
`claude-code`. `--model` picks woollama's backend (`fast`/`local`/`code`/`big`, or any
live `<provider>/<model>` id — tab-complete lists what woollama serves). `--depth` (on
`think`) swaps the prompt's prefix. Other verbs have their own adverbs — `search
--engine=`, `notify --urgency=`. `goo describe <verb>` shows which a verb takes.

---

## 7. Noun-first and repeat

Two shortcuts that match how you actually work:

```
$ goo do :app/firefox      # name the thing FIRST, then see/pick its verbs
$ goo do :app/firefox close   # …or run one (a pure reorder of `goo close :app/firefox`)

$ goo again                # repeat your last verb+adverbs (on the same kind of subject)
$ goo again :repo:woollama # …or on a NEW subject
```

`goo what <subject>` lists a thing's verbs (recently-used first); `goo forget` clears the
history `goo again` reads.

---

## 8. Tab completion

With completion installed (`source ~/.bashrc`, or `make install-completion`), TAB walks
every stage:

```
goo <TAB>                  # subcommands + all verbs
goo critique --<TAB>       # → --via=  --model=
goo critique --model=<TAB> # → fast local code big  + live woollama ids (ollama/…, woollama/…)
goo activate <TAB>         # → running apps (bare-positional handle completion)
goo switch :<TAB>          # → :app: :bt: :br: :ctr: :file: :hist: :mnt: :net: :ps: :repo: :sink: :ssh: :svc: :tmux: :win: :ws: …
goo switch :ws/<TAB>       # → :ws/0:0  :ws/0:1  :ws/1:0
```

---

## 9. The compose launcher

`goo-compose-gui` is the gnome-do/Kupfer surface for the same grammar — a keyboard-first
floating launcher. Type to filter a **Subject**, then a **Verb**, then an **Object** (for
two-step verbs), tweak any **adverbs**, and watch the exact `goo …` command assemble live
at the bottom before you run it. On run it shows the result inline (or, on failure, the
error with retry/edit/cancel). Build and try it with `make run-gui`.

(The `goo compose` CLI subcommand drives the same engine non-interactively from a scripted
answer queue — `GOO_COMPOSE_ANSWERS`, one choice per line — for automation and tests.)

---

## 10. Make your own

A plugin is a TOML file. The smallest useful one:

```toml
# ~/.config/cosmic-goo/plugins/shout.toml
name = "shout"

[[verbs]]
name = "loud"
accepts = ["text/*"]
cmd = "tr a-z A-Z <<< {subject.text|q}"
```

```
$ goo loud "make it loud"
MAKE IT LOUD
```

`{subject.text|q}` is a template substitution with the `|q` filter (shell-quote — safe
against any content). A plugin can declare verbs, types, **sources** (new addressable
domains), and adverbs. Full guide: [plugin-authoring](plugin-authoring.md). Validate after
editing:

```
$ goo validate
goo validate: OK (31 plugins, 20 types, 21 sources, 92 verbs, 5 adverbs, …)
```

---

## Where to go next

- [cli-reference](cli-reference.md) — every subcommand, addressing form, and completion stage
- [plugin-authoring](plugin-authoring.md) — types, sources, verbs, adverbs, sigils, filters
- [examples/ms-natural-4000-bindings](examples/ms-natural-4000-bindings.md) — a worked keyboard binding layout
- [limitations](limitations.md) — what's not built yet
