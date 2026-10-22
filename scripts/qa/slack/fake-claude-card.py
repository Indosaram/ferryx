#!/usr/bin/env python3
"""Draws a recorded Claude Code approval card and logs every key the pane receives.
Stand-in for a real Claude/Codex card when neither CLI is installed on this box."""
import os, sys, termios, tty, time
screen = open(os.path.join(os.path.dirname(__file__), "claude-approval.screen")).read()
log = open(os.environ.get("FAKE_CARD_LOG", "/tmp/fake-card-keys.log"), "a", buffering=1)
sys.stdout.write("\x1b[2J\x1b[H" + screen.rstrip("\n").replace("\n", "\r\n")); sys.stdout.flush()
fd = sys.stdin.fileno(); old = termios.tcgetattr(fd); tty.setraw(fd)
try:
    while True:
        b = os.read(fd, 64)
        if not b: break
        log.write(f"{time.time():.3f} {b!r}\n")
        if b in (b"\x03",): break
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
