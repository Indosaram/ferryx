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
