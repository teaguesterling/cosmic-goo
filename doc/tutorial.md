# Tutorial: learn `goo` by example

Every block below is runnable. Lines starting with `$` are commands; the line(s) under them are the expected output. Work top to bottom — each section builds on the last.

> Setup: from a checkout, either symlink the binary (`ln -s "$PWD/bin/goo" ~/.local/bin/goo`) or just call `./bin/goo`. The examples write `goo`.

---

## 1. The sentence: verb + subject

`goo` runs a **verb** on a **subject**. The simplest subject is literal text:

```
$ goo upper "hello world"
HELLO WORLD

$ goo wc "one two three"
      1       3      14

$ goo sha256 "hello"
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
```

`upper`, `wc`, `sha256` are verbs from the `text-utilities` plugin. They accept `text/*`, so any text subject works.

See what's available:

```
$ goo plugins          # the 24 loaded plugins
$ goo describe upper    # one verb's details
verb: upper
description: Convert to UPPERCASE
accepts: text/*
cmd: tr a-z A-Z <<< {subject.text|q}
provided by plugin: text-utilities
```

---

## 2. Where does the subject come from?

If you don't give a positional argument, `goo` finds a subject automatically — **stdin (if piped) → PRIMARY selection → clipboard → focused app**.

```
$ echo "piped text" | goo upper
PIPED TEXT

$ goo upper                # no arg, no pipe → uses your PRIMARY selection
                           # (highlight some text first, then run this)
```

That fallback is what makes `goo` good for keybindings: bind a key to `goo critique --via=clipboard` and it acts on whatever you've selected.

---

## 3. Addressing: pointing at specific things

A subject can be more than literal text. The shapes:

```
$ goo wc ./README.md            # a FILE (./ ~/ / are read as files — contents, not the path)
$ goo open https://x.com        # a URL (scheme:// is recognized; `open` handles files AND links)
$ goo upper ^                    # ^ = the clipboard (built-in → goo://clip/)
$ goo activate :app:firefox      # :dom:query — SEARCH the apps domain for "firefox" (fuzzy)
$ goo switch :ws/0:1             # :dom/path — the EXACT workspace value 0:1
```

Everything rewrites to one canonical `goo://<domain>/<path>` URI — see [cli-reference](cli-reference.md#subject-addressing). The two everyday rules:

- **Files and URLs need no sigil** — `./notes.md` and `https://…` are recognized by shape. `+x` forces literal text.
- **`:dom:query` searches (fuzzy), `:dom/id` is the exact value.** Either reaches anything a domain lists — discover them with `goo list`:

```
$ goo list apps | jq -r '.[].id'
Alacritty
Claude
...

$ goo list workspaces | jq -c '.[] | {id, title}'
{"id":"0:0","title":"ws-1 on DP-3"}
{"id":"0:1","title":"ws-2 on DP-3"}
```

**Skip the verb entirely.** If you give just an address and no verb, `goo` runs that type's *default* action — the CLI form of the protocol's `GOO` verb:

```
$ goo goo://br/main      # no verb → `log` (the git-branch type's default_for)
$ goo ~/notes.md         # → the file default verb (open)
```

(If a type has no default verb, `goo` says so rather than guessing.)

---

## 4. Adverbs: modifying *how* a verb runs

Some verbs take **adverbs** — `--name=value` modifiers. The classic is `--via`, which routes a text verb's prompt somewhere. Route to the clipboard first — it always works (no daemon) and *shows you* the assembled prompt:

```
$ goo critique "this paragraph could be tighter" --via=clipboard
$ wl-paste | head -3
You are providing expert review of the following passage.
Deduce the desired intent and tone, then critique accordingly.

$ goo think "recursion as a teaching device" --depth=ultra --via=clipboard
$ wl-paste | head -1
Ultrathink: exhaustively analyze every angle of the following passage:
```

**By default**, though, `--via=woollama` sends that prompt to your local [woollama](https://github.com/teaguesterling/woollama) router and prints the model's reply (so `goo summarize "…"` just answers — provided woollama is running):

```
$ goo summarize "the mitochondria is the powerhouse of the cell"
Mitochondria produce the cell's ATP through respiration.
```

`--via` values: `woollama` (default — needs the woollama daemon), `clipboard` (assemble the prompt, no LLM), `claude-desktop`, `claude-code`. The `--model` adverb picks woollama's backend — aliases (`fast`/`local`/`code`/`big`) or any live `<provider>/<model>` id (tab-complete lists what woollama serves). `--depth` (on `think`) swaps the prompt's prefix. Tab-complete shows the options — see §7.

```
$ goo describe think
verb: think
accepts: text/*
uses_adverbs: via, depth, model
...
```

---

## 5. Two-step verbs (subject + object)

A few verbs take an **object** as a second argument:

```
$ goo move-to :app:Alacritty :ws:0:1   # move an app (subject) to a workspace (object)
```

`move-to` accepts an app and an `object_type` of workspace; both go through the same addressing.

---

## 6. A tour of the plugins

```
$ goo calc "2 + 2 * 10"
22

$ goo qr-encode "https://example.com"     # a QR code, drawn in your terminal
█████████████████████████████████
████ ▄▄▄▄▄ █▄▀▀▄▄█▄█▀█ ▄▄▄▄▄ ████
...

$ goo qr-save "wifi-password-here"        # → a PNG, prints the path
/tmp/goo-qr-Ab3xZ9.png

$ goo scan-qr-image /tmp/goo-qr-Ab3xZ9.png # decode it back
wifi-password-here

$ goo status :repo:cosmic-goo          # `status` (polymorphic; dispatches to git for repos, Rust engine)
## main

$ goo now-playing                          # playerctl (no subject)
$ goo volume-up                            # wpctl (no subject)
$ goo notify "build done" --urgency=normal # desktop notification
```

Interactive capture verbs (need `slurp` to drag-select a region):

```
$ goo capture-region    # select an area → image on the clipboard
$ goo ocr-region        # select an area → OCR'd text to stdout
$ goo scan-qr           # select an area → decode a QR on screen
```

No-subject system verbs (the destructive ones confirm first):

```
$ goo lock              # loginctl lock-session
$ goo suspend           # confirms, then systemctl suspend
```

---

## 7. Tab completion

With completion installed (`source ~/.bashrc`, or `make install-completion`), TAB walks every stage:

```
goo <TAB>                  # subcommands + all verbs
goo critique --<TAB>       # → --via=  --model=
goo critique --via=<TAB>   # → claude-code  claude-desktop  clipboard  woollama
goo critique --model=<TAB> # → fast local code big  + live woollama ids (ollama/…, woollama/…)
goo activate <TAB>         # → running apps (bare-positional handle completion)
goo switch :<TAB>          # → :app: :bt: :clip: :file: :hist: :net: :repo: :sel: :sink: :svc: :tmux: :ws:
goo switch :ws:<TAB>       # → :ws:0:0  :ws:0:1  :ws:1:0  :ws:1:1
```

(Completion only fires when `goo` is on `$PATH`.)

---

## 8. The compose dialog

`goo compose` builds the whole sentence step by step — Subject → Verb (type-filtered) → Object (if any) → Adverbs → confirm → run.

The `goo` CLI itself is **non-interactive** — it never opens a window; it drives compose only from a scripted answer queue (`GOO_COMPOSE_ANSWERS`, one choice per line — for automation and tests). The **interactive**, picker-driven version (fuzzel/rofi/wofi/fzf) lives in the bash engine, `bin/goo compose`, and ahead in the native libcosmic `goo-compose` dialog. Bind *that* to a key for a "summon a launcher" feel.

---

## 9. Make your own

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

`{subject.text|q}` is a template substitution with the `|q` filter (shell-quote — safe against any content). Full authoring guide: [plugin-authoring](plugin-authoring.md). Validate after editing:

```
$ goo validate
goo validate: OK (24 plugins, ...)
```

---

## Where to go next

- [cli-reference](cli-reference.md) — every subcommand, addressing form, and completion stage
- [plugin-authoring](plugin-authoring.md) — types, sources, verbs, adverbs, sigils, filters
- [examples/ms-natural-4000-bindings](examples/ms-natural-4000-bindings.md) — a worked keyboard binding layout
- [limitations](limitations.md) — what's not built yet
