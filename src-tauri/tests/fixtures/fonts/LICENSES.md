# Fixture fonts for renderer unit tests

`freetype_raster` registers this directory with fontconfig in test builds only, so the Linux
rasterization tests do not depend on the fonts a host happens to have installed. Nothing here
ships in the app bundle.

| File | Source | License |
| --- | --- | --- |
| `NotoSansKR-Hangul-VF.ttf` | `ofl/notosanskr/NotoSansKR[wght].ttf` from google/fonts@6c1dbe27dcf7d817d4b62cec033ba77fa8e2a5ec (sha256 `194018e6...e252`), subset to U+AC00-BBA0, U+C2E4, U+D7A3 with the `wght` axis and named instances kept | SIL Open Font License 1.1, `OFL-NotoSansKR.txt` |
| `MesloLGS-NF-Regular-subset.ttf` | `MesloLGS NF Regular.ttf` from romkatv/powerlevel10k-media@145eb9fbc2f42ee408dacd9b22d8e6e0e553f83d (sha256 `d9794618...e57d`), subset to U+0020-007E, U+2500-257F, U+E0A0-E0D7, U+F418 | Apache License 2.0, `LICENSE-MesloLGS-NF.txt` |

Both subsets were made with fontTools 4.60.1 `pyftsubset --no-hinting --layout-features=
--name-IDs=* --name-languages=*` (the Noto subset also drops DSIG, GSUB and GPOS).

## Nerd Fonts glyphs in the Meslo subset

MesloLGS NF is Meslo patched by Nerd Fonts. Besides Meslo's own glyphs (Apache License 2.0),
the subset keeps these patched glyphs, each under its upstream source's MIT license:

| Code points | Glyph source | License |
| --- | --- | --- |
| U+E0A0-E0A2, U+E0B0-E0B3 | Powerline Symbols (powerline/powerline) | MIT, `LICENSE-Powerline.txt` (from powerline/powerline@11808cbe5c16e4621edcaefa916b5add81eab799 `LICENSE`) |
| U+E0A3, U+E0B4-E0D7 | Powerline Extra Symbols (ryanoasis/powerline-extra-symbols) | MIT, `LICENSE-Powerline-Extra-Symbols.txt` (from ryanoasis/powerline-extra-symbols@ae05de7c51f6609479f4f1a4a0f6f65631731c1b `LICENSE`) |
| U+F418 | Octicons (primer/octicons) | MIT, `LICENSE-Octicons.txt` (from primer/octicons@9c3c9d7426881584f6f507f3f4a54aea5879648e `LICENSE`) |

`LICENSE-MesloLGS-NF.txt` carries Meslo's copyright line and the full Apache License 2.0 text
from https://www.apache.org/licenses/LICENSE-2.0.txt.
