#!/usr/bin/env bash
# Single source of truth for where a build host keeps its clone, worktrees and
# cargo target. Nothing here hardcodes a real path: every root is derived from
# $HOME plus an optional override, so the same script is correct on macOS,
# Linux and Windows (Git Bash / MSYS) hosts.
#
# Usage:
#   scripts/host-paths.sh canonical        # the one long-lived clone
#   scripts/host-paths.sh worktree-root    # parent dir for throwaway worktrees
#   scripts/host-paths.sh worktree <slug>  # worktree path for a slug
#   scripts/host-paths.sh target           # shared cargo target dir
#   scripts/host-paths.sh evidence <slug>  # per-slug evidence dir
#   scripts/host-paths.sh env              # export statements for eval
#
# Overrides (all optional):
#   FERRYX_CANONICAL_ROOT  parent holding the canonical clone
#   FERRYX_WORKTREE_ROOT   parent holding worktrees
#   FERRYX_TARGET_DIR      shared cargo target
#
# Slug rule: <topic>-<yyyymmdd>, lowercase, dashes only.

set -euo pipefail

die() { printf 'host-paths: %s\n' "$1" >&2; exit 2; }

os_kind() {
  case "$(uname -s)" in
    Darwin) echo macos ;;
    Linux) echo linux ;;
    MINGW*|MSYS*|CYGWIN*) echo windows ;;
    *) die "unsupported platform: $(uname -s)" ;;
  esac
}

canonical() {
  git -C "$(dirname "${BASH_SOURCE[0]}")/.." rev-parse --show-toplevel 2>/dev/null && return
  case "$(os_kind)" in
    macos)   printf '%s/project/ferryx\n' "$HOME" ;;
    linux)   printf '%s/code/ferryx\n' "$HOME" ;;
    windows) printf '%s/ferryx\n' "${USERPROFILE:-$HOME}" ;;
  esac
}

worktree_root() {
  if [ -n "${FERRYX_WORKTREE_ROOT:-}" ]; then
    printf '%s\n' "${FERRYX_WORKTREE_ROOT%/}"
    return
  fi
  local c; c="$(canonical)"
  printf '%s/%s-wt\n' "$(dirname "$c")" "$(basename "$c")"
}

target_dir() {
  if [ -n "${FERRYX_TARGET_DIR:-}" ]; then
    printf '%s\n' "${FERRYX_TARGET_DIR%/}"
    return
  fi
  printf '%s/.target\n' "$(worktree_root)"
}

validate_slug() {
  case "$1" in
    '') die 'slug is required' ;;
    *[!a-z0-9-]*) die "slug must be lowercase ascii, digits and dashes only: $1" ;;
  esac
}

cmd="${1:-}"
case "$cmd" in
  canonical)      canonical ;;
  worktree-root)  worktree_root ;;
  target)         target_dir ;;
  worktree)
    slug="${2:-}"; validate_slug "$slug"
    printf '%s/%s\n' "$(worktree_root)" "$slug" ;;
  evidence)
    slug="${2:-}"; validate_slug "$slug"
    printf '%s/.evidence/%s\n' "$(worktree_root)" "$slug" ;;
  env)
    printf 'FERRYX_CANONICAL=%q\n' "$(canonical)"
    printf 'FERRYX_WORKTREE_ROOT=%q\n' "$(worktree_root)"
    printf 'FERRYX_TARGET_DIR=%q\n' "$(target_dir)"
    ;;
  ''|-h|--help|help)
    sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) die "unknown command: $cmd" ;;
esac
