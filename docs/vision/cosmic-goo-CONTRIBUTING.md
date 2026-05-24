# Contributing to cosmic-goo

Thanks for your interest. This guide is organized by what kind of contribution you're making.

---

## Reporting bugs and feature requests

File a GitHub issue with:

- What you tried (`goo critique --via=fabric "..."` etc.)
- What you expected
- What happened (error message, hang, wrong output)
- Output of `goo validate` and `goo plugins`
- Output of `recon/env.sh` if it's an environment issue

---

## Writing a plugin

This is the most valuable contribution path. A plugin is a TOML file (and optionally helper scripts) that adds types, sources, verbs, or adverbs to cosmic-goo.

### Quick start

The smallest useful plugin is a few lines:

```toml
# ~/.config/cosmic-goo/plugins/my-thing.toml
name = "my-thing"
description = "A one-line description"

[[verbs]]
name = "uppercase"
accepts = ["text/*"]
cmd = "echo '{subject.text}' | tr a-z A-Z"
```

After saving, `goo plugins` should list it, `goo describe uppercase` should show its details, and `goo uppercase "hello world"` should print `HELLO WORLD`.

### Plugin discovery order

cosmic-goo loads from these directories in order (later wins for name collisions):

1. `/usr/share/cosmic-goo/plugins/` — system-installed
2. `/etc/cosmic-goo/plugins/` — system admin overrides
3. `~/.config/cosmic-goo/plugins/` — user plugins
4. `$PWD/.cosmic-goo/plugins/` — project-local overrides

A plugin is either a single TOML at `plugins/<name>.toml` or a directory `plugins/<name>/` containing `plugin.toml` plus binaries/scripts.

### What a plugin can contribute

| Section | Purpose | Required fields |
|---------|---------|----------------|
| `[[types]]` | Declare a vendor MIME for handle objects | `name`, `display`, `kind` |
| `[[sources]]` | A place to find typed objects | `name`, `emits`, `list_cmd` |
| `[[verbs]]` | A named action | `name`, `accepts`, `cmd` OR `prompt`+`uses_adverbs` |
| `[[adverbs]]` | A modifier on verbs | `name`, `kind`, scope (`applies_to` or `applies_to_verbs`) |

### Types

If your tool produces a kind of object that doesn't yet have a MIME type, declare a vendor MIME in a namespace you own:

```toml
[[types]]
name = "application/vnd.your-tool.thing"
display = "your-tool thing"
kind = "handle"     # "handle" = system entity; "content" rarely needed (use real MIME)
```

Vendor namespaces are first-come-first-served by convention. Use `application/vnd.<your-tool-name>.<type>` and don't squat on names you don't maintain.

### Sources

A source lists typed objects. The `list_cmd` is a shell command that produces JSON to stdout. The JSON must be an array of objects with at minimum `id` and `title` fields:

```json
[
  {"id": "stable-id-1", "title": "Display name", "subtitle": "Optional info", "metadata": {}},
  {"id": "stable-id-2", "title": "Another", "metadata": {"path": "/home/..."}}
]
```

The `id` must be stable across invocations (this is what verbs receive as `{subject.id}`). `metadata` is opaque to cosmic-goo but available to verb templates as `{subject.metadata.<field>}`.

```toml
[[sources]]
name = "my-things"
prefix = "thing"           # optional: enables :thing scoping in launcher
icon = "applications-other" # icon-theme name
emits = "application/vnd.your-tool.thing"
list_cmd = "your-tool list --json"
preview_cmd = "your-tool show {subject.id}"  # optional, shown in launcher/dialog
```

### Verbs

Verbs are the actions. They accept input types and produce a command (or a prompt routed through an adverb).

**Simple verb** with a direct command:

```toml
[[verbs]]
name = "uppercase"
accepts = ["text/*"]
cmd = "echo {subject.text} | tr a-z A-Z"
```

Template variables:
- `{subject.text}` — for text-typed subjects, the content
- `{subject.id}` — for handle-typed subjects, the ID
- `{subject.metadata.<field>}` — arbitrary metadata field
- `{object.<...>}` — same fields, on the object (for two-step verbs)
- `{cwd}` — current working directory
- `{verb.<field>}` — fields from the verb's own definition

**Two-step verb** taking an object:

```toml
[[verbs]]
name = "move-to"
accepts = ["application/vnd.cos-cli.app"]
object_type = "application/vnd.cos-cli.workspace"
cmd = "cos-cli move --app-id {subject.id} --workspace {object.index}"
```

**Adverb-routed verb** using a `prompt` instead of a `cmd`:

```toml
[[verbs]]
name = "critique"
accepts = ["text/*"]
uses_adverbs = ["via"]    # the `via` adverb is provided by claude-routing plugin
fabric_pattern = "analyze_claims"
prompt = """
You are providing expert review of the following passage.
Deduce the desired intent and tone, then critique accordingly.

---
{subject.text}
"""
```

The `via` adverb will pick the actual template (Fabric, Claude Desktop, Claude Code, or clipboard) and use `{verb.prompt}` and `{verb.fabric_pattern}` from this verb.

**Destructive verb** with confirmation:

```toml
[[verbs]]
name = "delete"
accepts = ["application/vnd.your-tool.thing"]
cmd = "your-tool delete {subject.id}"
confirm = true     # prompts before executing
```

### Adverbs

Adverbs modify how verbs execute. Two flavors:

**Selector adverb** — picks from a known set:

```toml
[[adverbs]]
name = "depth"
kind = "selector"
applies_to_verbs = ["think"]      # scope by verb name
# or: applies_to = ["text/*"]      # scope by input type
default = "normal"

[adverbs.depth.values.normal]
template_var = { depth_prefix = "Think about" }

[adverbs.depth.values.deeply]
template_var = { depth_prefix = "Deeply consider" }
```

`template_var` injects variables that the verb's prompt/cmd template can use.

**Fill adverb** — takes a free-form value:

```toml
[[adverbs]]
name = "name"
kind = "fill"
applies_to_verbs = ["rename", "create-scene"]
prompt = "New name:"      # shown in dialog/launcher when prompting
```

The user-supplied value is available in the verb template as `{adverbs.name}`.

### Helper scripts

If your plugin needs more than a one-liner, put scripts in a directory:

```
~/.config/cosmic-goo/plugins/my-thing/
├── plugin.toml
└── bin/
    ├── my-thing-list.sh
    └── my-thing-do.sh
```

Reference them in TOML via relative paths from the plugin directory:

```toml
[[sources]]
list_cmd = "{plugin.dir}/bin/my-thing-list.sh"
```

### Validating

Always run `goo validate` after editing a plugin. It checks:
- TOML syntax
- Required fields
- Type references (no orphan accepts)
- Adverb scope (no dangling `applies_to_verbs` references)
- Vendor namespace conflicts

---

## Code contributions

### Shell code (everything in v1)

- Bash 5+ required (associative arrays, modern globs)
- ShellCheck clean (`make shellcheck` runs against `lib/` and `bin/`)
- Run all tests before submitting: `make test` (uses bats-core)
- 2-space indent; functions over inline code; quote everything

### Rust code (for `goo-compose` and eventually the meta-plugin)

- Stable Rust, `cargo fmt` clean, `cargo clippy --all-targets -- -D warnings` passes
- Prefer libcosmic + iced for UI; avoid framework sprawl

### Tests

- Shell: bats-core, files in `tests/*.bats`
- Rust: `cargo test`
- Integration: `tests/integration/` runs full plugin flows against a mock environment

### Commits and PRs

- Conventional commits preferred (`feat:`, `fix:`, `docs:`, etc.)
- One logical change per PR
- For plugin additions, include the plugin TOML and at least one test case in `tests/plugins/`
- For architectural changes, open an issue for discussion first

---

## Documentation contributions

- Plugin examples in `doc/examples/plugins/` are always welcome — even for tools maintainers don't use
- Binding examples for keyboards/layouts not yet covered are also welcome
- Spelling and clarity fixes anywhere

---

## What we're NOT looking for (yet)

- Performance optimizations before functional correctness
- Plugin sandboxing/signing (architecturally deferred — see Open Questions in the spec)
- Alternative UIs that bypass pop-launcher (we want the integration to work first)
- New top-level languages (we have shell and Rust; that's enough for v1)

---

## Code of conduct

Be excellent to each other. Disagree respectfully on technical merits. No harassment, ad hominem, or bad-faith argumentation. Maintainers reserve the right to remove comments and contributors who don't meet this bar.
