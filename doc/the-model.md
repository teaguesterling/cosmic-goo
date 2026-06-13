# The model

goo is a **grammar of operations**. You name a thing on your desktop, then say what to
do with it. The thing can be an app, a window, a file, a git repo, a Bluetooth device, a
workspace, an audio sink, a clipboard entry — or some text. The *same* grammar drives all
of them.

## A goo sentence

```
goo <verb> <subject> [<object>] [--adverb=value …]
```

- **subject** — *what* you're acting on (an app, a file, a repo…). The noun.
- **verb** — *what to do* (`activate`, `open`, `status`, `connect`, `summarize`…).
- **object** — a second noun some verbs need (move-to *a workspace*; rename *to a name*).
- **adverbs** — `--key=value` modifiers (`--model=big`, `--via=clipboard`, `--depth=ultra`).

```sh
goo activate :app/firefox            # focus an app
goo status :repo:cosmic-goo          # short git status of a repo (fuzzy-match the name)
goo move-to :app:alacritty :ws/0:1   # move an app to a workspace   (verb + subject + object)
goo connect :bt/headphones           # connect a Bluetooth device
goo open ./notes.md                  # open a file in its default app
```

That's it. Everything else is filling in the blanks.

## A subject is any desktop thing

You address a subject with a **sigil** — a short prefix that names a domain. `:app:` is the
running-apps domain, `:repo:` is your git repos, and so on. Two forms:

- `:domain/exact-id` — a specific thing (`:app/firefox`, `:ws/0:1`).
- `:domain:query` — a fuzzy search in that domain (`:repo:goo`, `:bt:head`).

| sigil | addresses | sigil | addresses |
|---|---|---|---|
| `:app:` | running app | `:bt:` | Bluetooth device |
| `:win:` | a window | `:net:` | network connection |
| `:ws:` | workspace | `:ssh:` | SSH host (from `~/.ssh/config`) |
| `:repo:` | git repository | `:svc:` | systemd user service |
| `:br:` | git branch | `:ctr:` | container (docker/podman) |
| `:file:` | file in the cwd | `:sink:` | audio output |
| `:recent:` | recently-opened file | `:tmux:` | tmux session |
| `:mnt:` | mount point | `:ps:` | process |
| `:hist:` | clipboard-history entry | `:emo:` | emoji |
| `:sel:` | the PRIMARY selection (text) | `:clip:` / `^` | the clipboard (text) |

Native shapes need no sigil: `./file` `~/file` `/abs` resolve to files, `https://…` to a
URL, and bare words to text (or use `+word` to force literal text). Anything a plugin can
list, goo can address — `goo plugins` shows what's loaded; a custom plugin adds new domains.

## One grammar, every thing

Two properties make it *one* grammar rather than a pile of commands:

**Verbs are polymorphic.** `connect` means the right thing for whatever you hand it —
`goo connect :bt/headphones` pairs Bluetooth, `goo connect :ssh/prod` opens an SSH session,
`goo connect :net/home-vpn` brings up a connection. Same for `status`, `logs`, `info`,
`open`. goo picks the most specific implementation for the subject's type.

**Types coerce.** A verb that wants JSON will still run on a CSV — goo finds a conversion
route and takes it:

```sh
$ goo --explain json-keys data.csv
subject: text/csv (via libmagic)
text/csv → csv2json → application/json → (json-keys) → …
```

You asked for `json-keys`; goo noticed the file was CSV and plans the route through
`csv2json` before the verb. `--explain` shows that plan (it doesn't run anything); running
it for real needs the channel's converter tool (here `mlr`) — if it's missing you get a
clean `415` that names it, never a wrong-type run. `--paths` lists every route.

## Discovering what you can do

- **`goo what <subject>`** — list the verbs that apply to a thing (most-recent first).
- **`goo do <subject>`** — noun-first: name the thing, then pick a verb. `goo do :app/firefox`
  lists its verbs; `goo do :app/firefox close` runs one (a pure reorder of `goo close …`).
- **`goo <subject>`** with no verb runs the type's **default** (an app → `activate`, a repo →
  `status`, a file → `open`).
- **The compose dialog** (`goo-compose-gui`) is the same grammar as a keyboard-first,
  gnome-do-style launcher: type to filter a Subject, then a Verb, then an Object, tweak
  adverbs, and watch the exact `goo …` command assemble live before you run it.
- **`goo again`** repeats your last verb+adverbs on a new subject.

## Text is one of the things

Text is just another subject type — so the same grammar gives you a family of text verbs:
`upper`, `wc`, `base64-encode`, `sha256`, `calc`, and LLM verbs (`summarize`, `critique`,
`think`, `draft-response`) that route through a local model. With no subject, text verbs
borrow the PRIMARY selection or clipboard, so `goo summarize` summarises whatever you've
highlighted. The routing (`--via`) and model (`--model`) are just adverbs — the same slot
machinery every other verb uses.

→ Ready to try it: the **[Quickstart](quickstart.md)** runs you through address → verb →
coerce → route in five minutes.
