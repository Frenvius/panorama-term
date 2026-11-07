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

## Limits

- Plugins only change what the extension points expose. They cannot restyle or replace
  parts of the app.
- Plugins apply to the desktop app only, not to the phone app.
