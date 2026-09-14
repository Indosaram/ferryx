# Provider fixture diagnosis resolved

Earlier width-limited ps hypothesis was NOT established. `-wwE` did not fix the fixture; the one-line ipc/agents.rs change was reverted. No net production discovery change remains.

The real `/bin/sh` and `/bin/zsh` descendant fixtures had matching argv and verified PI_SESSION_FILE inside their shell, but macOS ps exposed no environment (even unbounded explicit command format). A user-installed /opt/homebrew/bin/bash fixture exposes the environment and existing owner discovery accepts it. A12-session-metadata-GREEN-provider-user-process.log reaches accepted provider, predecessor HTTP/IPC retained metadata, exit retention, all cleanup, and test result 1 passed.

Final verification must run again with original ps flags, now restored. Provider fixture executable should be explicitly supplied by the isolated runner rather than hardcoded production behavior; production does not depend on Homebrew Bash.
