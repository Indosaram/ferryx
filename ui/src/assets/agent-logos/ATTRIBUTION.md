# Agent tab logo attribution

Most files in this directory are copied from
[`@lobehub/icons-static-svg` 1.94.0](https://www.npmjs.com/package/@lobehub/icons-static-svg),
distributed under the [MIT License](https://github.com/lobehub/lobe-icons/blob/master/LICENSE).

They are bundled locally so tab rendering never depends on a network request.

Logos are used verbatim from their source: brands that publish a full-color mark keep
those exact colors, and brands whose official mark is a single flat color are listed as
adaptive below so they invert for dark chrome. Never hand-tint a brand logo.

| File | Brand asset | Source | Color |
| --- | --- | --- | --- |
| `antigravity.svg` | Antigravity | lobehub `antigravity-color` | brand color |
| `claude.svg` | Claude | lobehub `claude-color` | brand color |
| `codex.svg` | OpenAI Codex | lobehub `codex-color` | brand color |
| `gemini.svg` | Google Gemini | lobehub `gemini-color` | brand color |
| `copilot.svg` | GitHub Copilot | lobehub `copilot-color` | brand color |
| `kimi.svg` | Kimi | lobehub `kimi-color` | brand color |
| `opencode.svg` | OpenCode | lobehub `opencode` | adaptive monochrome |
| `pi.svg` | Pi | lobehub `pi` | adaptive monochrome |
| `cursor.svg` | Cursor | lobehub `cursor` | adaptive monochrome |
| `grok.svg` | Grok | lobehub `grok` | adaptive monochrome |
| `cline.svg` | Cline | lobehub `cline` | adaptive monochrome |
| `omo.svg` | [Official OMO light icon](https://github.com/code-yeongyu/oh-my-openagent/raw/dev/.github/assets/omo-icon-light.svg) | official repo | adaptive monochrome (backing plate removed) |
| `aider.png` | Aider | [Aider-AI/aider `android-chrome-192x192.png`](https://github.com/Aider-AI/aider/blob/main/aider/website/assets/icons/android-chrome-192x192.png), Apache-2.0 | brand color |
| `crush.png` | Charm Crush | [charmbracelet/crush `crush-icon-solo.png`](https://github.com/charmbracelet/crush/blob/main/internal/ui/notification/crush-icon-solo.png), MIT | brand color |
| `droid.svg` | Factory Droid | [factory.ai `favicon.svg`](https://factory.ai/favicon.svg) | adaptive monochrome (backing plate removed) |
| `gjc.png` | Gajae-Code (GJC) | [Yeachan-Heo/gajae-code `assets/character.png`](https://github.com/Yeachan-Heo/gajae-code/blob/main/assets/character.png), MIT | brand color |

`aider.png` and `crush.png` are raster because neither project publishes a vector mark;
both are downscaled to 128x128 from the official artwork. `gjc.png` is the official
character mark with the bundled wordmark band cropped away, fitted to 128x128 on a
transparent canvas.

Do not add a logo here unless its source and license are recorded above. Agent types
without a vetted local logo deliberately use the terminal icon.
