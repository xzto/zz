---
title: Configuration
description: Appearance settings and tmux configuration with zz-specific overrides.
---

Keep zz-specific settings in two plain-text files:

| File | Format | Covers |
| --- | --- | --- |
| `~/.config/zz/config` | Ghostty-style `key = value` | appearance: fonts, colors, themes, padding, opacity |
| `~/.config/zz/mux.conf` | tmux syntax | overrides for prefix, key bindings, mux options |

Both are picked up without a restart. To split `config`, add an include at the point where you
want its settings applied:

```config
include = config.local
include = -optional.conf
```

The first line requires `config.local`; a missing file raises a load error. The `-` prefix lets zz
skip a missing file. Relative paths resolve beside the file containing the include. You can nest
includes up to 16 levels; zz rejects cycles. Changes to included files also trigger a reload.
Without an include, zz does not read `config.local`.

## One-shot import

On first launch zz offers to import any Ghostty or tmux configuration it finds. You can
choose another source path in Settings. Ghostty appearance becomes a snapshot in `zz/config`;
tmux commands become an import block in `zz/mux.conf`.

The daemon loads only `zz/mux.conf`. Explicit `zz -f <file>` arguments replace that file,
in argument order. Reload uses the same selection. Terminal's Appearance rows edit `zz/config`;
Multiplexer's Options rows edit `zz/mux.conf`, matching the editor on each page.

## Ghostty compatibility

The appearance layer speaks Ghostty: `theme` files, per-style font stacks
with OpenType features, `minimum-contrast`, per-edge padding, the full
256-color palette, and OSC 10/11/12 color queries answered, so
terminal-aware TUIs derive their colors correctly.

## Where every value came from

The settings UI shows where every value comes from (default, theme file,
Ghostty import, or your override), and a config-file reload never silently
reverts a choice you made in the UI. Unsupported directives in `mux.conf`
are reported with a diagnostic, not ignored.
