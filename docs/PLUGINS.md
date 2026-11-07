# Plugins

A plugin extends Panorama through the extension points the app offers. Most plugins
are declarative: a `plugin.json` plus the files it points to, no code.

## Where plugins live

Each plugin is a folder in the `plugins` folder of the Panorama config directory:

| Platform | Folder |
| --- | --- |
| Linux | `~/.config/panorama/plugins/` |
| macOS | `~/Library/Application Support/panorama/plugins/` |
| Windows | `%APPDATA%\panorama\plugins\` |

Dev builds use `panorama-dev` instead of `panorama`. **Settings → Plugins** lists the
installed plugins, shows the folder and opens it. Changes in the folder show up right
away, no restart needed. Plugins are on when installed and can be switched off there.

## The manifest

```json
{
  "id": "nord",
  "name": "Nord",
  "version": "1.0.0",
  "description": "Arctic, north-bluish colors.",
  "author": "Someone"
}
```

| Field | Required | Rules |
| --- | --- | --- |
| `id` | yes | lowercase letters, digits and dashes; must match the folder name |
| `name` | yes | shown in Settings |
| `version` | yes | `major.minor.patch`, optionally with a pre-release or build suffix |
| `description` | no | shown under the name |
| `author` | no | shown when there is no description |

A plugin whose manifest is missing or invalid is listed in Settings with the reason.

`contributes` (optional) lists what the plugin adds. Each kind is described below.

## Themes

```json
{
  "contributes": {
    "themes": [
      {
        "id": "dark",
        "label": "Nord Dark",
        "kind": "dark",
        "colors": { "surface-1": "#2e3440", "text-1": "#d8dee9", "accent": "#88c0d0" },
        "terminal": {
          "foreground": "#d8dee9",
          "cursor": "#d8dee9",
          "ansi": ["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
                   "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"]
        }
      }
    ]
  }
}
```

| Field | Required | Rules |
| --- | --- | --- |
| `id` | yes | lowercase letters, digits and dashes; unique within the plugin |
| `label` | yes | shown in Settings; by convention ends in "Dark" or "Light" |
| `kind` | yes | `dark` or `light` |
| `colors` | no | values for the app's CSS variables, without the leading `--` |
| `terminal.foreground` / `terminal.cursor` | no | default text and cursor color |
| `terminal.ansi` | no | the 16 ANSI colors (normal 0-7, then bright 8-15), used for text and backgrounds |

A theme is drawn on top of the built-in theme of its kind, so it only needs the values
it changes. **Settings → Appearance** shows a picker for the dark and the light theme
once a plugin adds one; *System* switches between the two. If the plugin is switched
off or removed, Panorama goes back to its own theme.

Colors a theme can set: `surface-1..3`, `border-1..3`, `text-1`, `text-strong`,
`text-muted`, `text-dim`, `accent`, `danger`, `run-glyph`, `run-fill`, `run-on-fill`,
`stop-fill`, `stop-on-fill`, `fill-hover`, `fill-press`, `hover-1..3`, `hover-soft`,
`pill-bg`, `minimap-bg`, `shadow-island`, `shadow-tile`, `shadow-pop`, `shadow-modal`,
and the code colors `shiki-foreground`, `shiki-token-comment`, `shiki-token-punctuation`,
`shiki-token-link`, `shiki-token-keyword`, `shiki-token-string`,
`shiki-token-string-expression`, `shiki-token-constant`, `shiki-token-parameter`,
`shiki-token-function`. Any other name makes the manifest invalid.

## Fonts

```json
{
  "contributes": {
    "fonts": [
      {
        "family": "Iosevka Term",
        "monospace": true,
        "files": ["fonts/regular.woff2", { "path": "fonts/bold.woff2", "weight": 700 }]
      }
    ]
  }
}
```

| Field | Required | Rules |
| --- | --- | --- |
| `family` | yes | letters, digits, spaces, dots, dashes or underscores; unique within the plugin |
| `monospace` | no | `true` lets the font be used for code and the terminal; default `false` |
| `files` | yes | font files inside the plugin folder (`.woff2`, `.woff`, `.ttf`, `.otf`) |
| `files[].weight` | no | `100` to `900`, default `400` |
| `files[].style` | no | `normal` or `italic`, default `normal` |

A file can be given as a plain path when it is the regular face. **Settings →
Appearance** shows a picker for the interface font, the code font (editor and diff)
and the terminal font once a plugin adds one. Only the chosen fonts are read, when
they are chosen. The terminal measures the chosen font, so columns keep matching the
tile width. If a font cannot be read, or its plugin is switched off, Panorama goes
back to its own font.

## Languages

```json
{
  "contributes": {
    "languages": [
      { "id": "mylang", "extensions": [".my"], "grammar": "syntaxes/mylang.tmLanguage.json" },
      { "id": "groovy", "extensions": [".jenkins"] }
    ]
  }
}
```

| Field | Required | Rules |
| --- | --- | --- |
| `id` | yes | lowercase letters, digits and `+ # . -`; unique within the plugin |
| `extensions` | no | file extensions like `.my` that use this language |
| `grammar` | no | a TextMate grammar (`.json`, with a `scopeName`) inside the plugin |

A language needs a `grammar`, `extensions`, or both:

- **With a grammar**, the plugin brings a language Panorama does not know. Files with its
  extensions, and fenced code blocks tagged with its `id`, are colored with it.
- **Without a grammar**, the extensions are mapped to a language Panorama already knows
  (`id` is that language, e.g. `groovy`).

Coloring follows the active theme's code colors, in the editor, the diff viewer and
Markdown code blocks. A grammar is read the first time a file needs it; changes to a
grammar apply to files opened afterwards.

## Limits

- Plugins only change what the extension points expose. They cannot restyle or replace
  parts of the app.
- Plugins apply to the desktop app only, not to the phone app.
- The terminal font keeps Panorama's size and line height and draws cell by cell, so
  it must be monospaced and ligatures are not shown.
- Languages bring coloring only: no completion, linting or other editor features.
- In the terminal, ANSI white (7) and bright white (15) arrive as the same color, so a
  theme cannot tell them apart; bright white wins.
