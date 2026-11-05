# Raw capture format

Raw diffs, numbered source snapshots, baseline source and the protocol diff log are gzip-compressed losslessly. Decompress with Python gzip or gzip -dc; filenames in reports refer to the uncompressed logical artifact. Initial staged git diff --check exited 2 on literal diff-context blank lines and numbered blank source lines, not product edits. Compression retains those exact bytes without adding whitespace errors to the evidence commit.
