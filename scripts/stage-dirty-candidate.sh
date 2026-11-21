#!/usr/bin/env bash
# usage: bash scripts/stage-dirty-candidate.sh manifest <outdir> [--base <rev>]
#        bash scripts/stage-dirty-candidate.sh verify   <outdir> <worktree-root>
# manifest writes candidate.patch, candidate.manifest, candidate.list.{a,b}, candidate.meta.
# verify recomputes sha256, git blob id and mode in <worktree-root> and compares.
# exit: 0 ok, 1 mismatch, 2 usage or refused. No network, no host claim, no commit.
set -u

die() { echo "stage-dirty-candidate: $*" >&2; exit 2; }
note() { echo "stage-dirty-candidate: $*" >&2; }

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P) || die "cannot resolve script directory"
repo=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null) || die "not inside a git work tree"

file_mode() {
  local m
  m=$(stat -f %Lp "$1" 2>/dev/null) || m=$(stat -c %a "$1" 2>/dev/null) || return 1
  printf '%04d\n' "$m"
}

file_sha256() {
  local s
  s=$(shasum -a 256 "$1" 2>/dev/null) || s=$(sha256sum "$1" 2>/dev/null) || return 1
  printf '%s\n' "${s%% *}"
}

stream_sha256() {
  local s
  s=$(shasum -a 256 2>/dev/null) || s=$(sha256sum 2>/dev/null) || return 1
  printf '%s\n' "${s%% *}"
}

usage() {
  sed -n '2,6p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

is_tracked() {
  git -C "$1" ls-files --error-unmatch -- "$2" >/dev/null 2>&1
}

disk_list() {
  local wt=$1 base=$2 out=$3
  shift 3
  {
    git -C "$wt" diff --name-only -z --diff-filter=ACMRTUD "$base" -- "$@" || return 1
    git -C "$wt" ls-files -o --exclude-standard -z -- "$@" || return 1
  } | LC_ALL=C sort -z -u >"$out" || return 1
  return 0
}

# Gitlinks (submodule pointers) are single index entries, not files, and they are the one thing a
# `git apply` on a fresh worktree cannot reproduce: the tree holds an initialized or empty directory
# there, while candidate.patch would carry a `Subproject commit <sha>` hunk that applies only if the
# receiving worktree already has the submodule checked out at a matching commit. So they are
# enumerated and excluded from the candidate everywhere, and a MOVED pointer is refused outright
# rather than silently dropped from the patch.
gitlink_list() {
  local wt=$1 base=$2 out=$3
  {
    git -C "$wt" ls-tree -r -z --full-tree "$base" | tr '\0' '\n' | awk -F'\t' '$1 ~ /^160000 /{print $2}' || return 1
    git -C "$wt" ls-tree -r -z --full-tree HEAD | tr '\0' '\n' | awk -F'\t' '$1 ~ /^160000 /{print $2}' || return 1
  } | LC_ALL=C sort -u >"$out" || return 1
  return 0
}

manifest_from_list() {
  local wt=$1 list=$2 out=$3 path mode sha blob
  : >"$out" || return 1
  while IFS= read -r -d '' path; do
    [ -n "$path" ] || continue
    case "$path" in
      *$'\n'*) note "refusing a path containing a newline"; return 1 ;;
    esac
    if [ -L "$wt/$path" ]; then
      note "refusing symlink: $path"
      return 1
    fi
    if [ ! -e "$wt/$path" ]; then
      printf 'DELETED - - %s\n' "$path" >>"$out" || return 1
      continue
    fi
    if [ ! -f "$wt/$path" ]; then
      note "refusing non-regular file: $path"
      return 1
    fi
    mode=$(file_mode "$wt/$path") || return 1
    sha=$(file_sha256 "$wt/$path") || return 1
    blob=$(git -C "$wt" hash-object -- "$path" 2>/dev/null) || return 1
    printf '%s %s %s %s\n' "$mode" "$sha" "$blob" "$path" >>"$out" || return 1
  done <"$list"
  LC_ALL=C sort -o "$out" "$out" || return 1
  return 0
}

patch_list() {
  local patch=$1 out=$2
  # BSD awk reads RS="\0" as paragraph mode, so split on NUL with tr/cut instead.
  # This is only correct because a NUL-framed rename record carries an empty path
  # field; cmd_manifest refuses renames before this runs.
  git apply --numstat -z "$patch" 2>/dev/null | tr '\0' '\n' | cut -f3- |
    LC_ALL=C sort -u | tr '\n' '\0' >"$out" || return 1
  return 0
}

manifest_paths_nul() {
  # The manifest sorts by mode first, so the paths must be re-sorted to compare
  # against a path-sorted list.
  sed 's/^[^ ]* [^ ]* [^ ]* //' "$1" | LC_ALL=C sort | tr '\n' '\0'
}

cmd_manifest() {
  [ $# -ge 1 ] || usage
  local outdir=$1; shift
  local base patch meta path drift bad
  base=$(git -C "$repo" rev-parse HEAD 2>/dev/null) || die "cannot resolve HEAD"
  while [ $# -gt 0 ]; do
    case "$1" in
      --base) shift; [ $# -ge 1 ] || die "--base needs a revision"; base=$1 ;;
      *) die "unknown argument: $1" ;;
    esac
    shift
  done
  git -C "$repo" rev-parse --verify --quiet "$base^{commit}" >/dev/null || die "not a commit: $base"
  [ -z "$(git -C "$repo" diff --cached --name-only)" ] || die "index has staged changes; worktree bytes only"
  [ -z "$(git -C "$repo" diff --name-only --diff-filter=R "$base")" ] ||
    die "candidate has staged renames; commit them or split the change, a rename patch is ambiguous here"

  [ -e "$outdir" ] && die "refusing to reuse $outdir: pass a fresh directory"
  mkdir -p "$outdir" || die "cannot create $outdir"
  patch=$outdir/candidate.patch
  meta=$outdir/candidate.meta

  # Gitlinks are excluded from the path set, the patch and the read-back, because no `git apply` can
  # reproduce a submodule pointer in a worktree that does not hold that submodule. A pointer that
  # MOVED relative to the base is refused: the alternative is a candidate whose patch omits a change
  # the tree actually contains, which is the failure mode this script exists to prevent.
  gitlink_list "$repo" "$base" "$outdir/candidate.gitlinks" || die "cannot enumerate gitlinks"
  local -a excl=()
  local gitlink moved=""
  while IFS= read -r gitlink; do
    [ -n "$gitlink" ] || continue
    excl+=(":(exclude,literal)$gitlink")
    if [ -n "$(git -C "$repo" diff --raw "$base" -- "$gitlink" 2>/dev/null)" ]; then
      moved="$moved\n  submodule pointer moved: $gitlink"
    fi
  done <"$outdir/candidate.gitlinks"
  case "$moved" in
    "") : ;;
    *)
      printf 'stage-dirty-candidate: REFUSED, the tree moves a submodule pointer:%b\n' "$moved" >&2
      echo "stage-dirty-candidate: candidate.patch cannot reproduce a gitlink in a fresh worktree." >&2
      echo "stage-dirty-candidate: commit the pointer move separately, or pin it out of the candidate." >&2
      exit 2
      ;;
  esac

  disk_list "$repo" "$base" "$outdir/candidate.list.a" ${excl[@]+"${excl[@]}"} || die "cannot list candidates"
  manifest_from_list "$repo" "$outdir/candidate.list.a" "$outdir/candidate.manifest.a" ||
    die "cannot build the first manifest"

  git -C "$repo" diff --binary --no-color --no-ext-diff "$base" -- ${excl[@]+"${excl[@]}"} >"$patch" ||
    die "cannot write $patch"
  while IFS= read -r -d '' path; do
    [ -n "$path" ] || continue
    is_tracked "$repo" "$path" && continue
    ( cd "$repo" && git diff --no-index --binary --no-color -- /dev/null "$path" ) >>"$patch" 2>/dev/null
    [ $? -le 1 ] || die "cannot diff untracked file: $path"
  done <"$outdir/candidate.list.a"

  disk_list "$repo" "$base" "$outdir/candidate.list.b" ${excl[@]+"${excl[@]}"} || die "cannot re-list candidates"
  manifest_from_list "$repo" "$outdir/candidate.list.b" "$outdir/candidate.manifest" ||
    die "cannot build the final manifest"
  patch_list "$patch" "$outdir/candidate.list.patch" || die "cannot read back $patch"

  drift=""
  cmp -s "$outdir/candidate.list.a" "$outdir/candidate.list.b" ||
    drift="$drift\n  path set changed between the first and last pass"
  cmp -s "$outdir/candidate.manifest.a" "$outdir/candidate.manifest" ||
    drift="$drift\n  content, mode or blob id changed between the two passes"
  cmp -s "$outdir/candidate.list.a" "$outdir/candidate.list.patch" ||
    drift="$drift\n  patch file set does not match the candidate path set"
  case "$drift" in
    "") : ;;
    *)
      {
        printf 'stage-dirty-candidate: REFUSED, the tree moved during the snapshot:%b\n' "$drift"
        echo "stage-dirty-candidate: nothing in $outdir is a valid candidate. Stop the writers and rerun into a fresh directory." >&2
        echo "stage-dirty-candidate: path set diff (disk vs patch):" >&2
        diff <(tr '\0' '\n' <"$outdir/candidate.list.a") <(tr '\0' '\n' <"$outdir/candidate.list.patch") >&2
        echo "stage-dirty-candidate: manifest diff (first vs last pass):" >&2
        diff "$outdir/candidate.manifest.a" "$outdir/candidate.manifest" >&2
      } >&2
      exit 2
      ;;
  esac

  {
    printf 'base %s\n' "$base"
    printf 'branch %s\n' "$(git -C "$repo" rev-parse --abbrev-ref HEAD 2>/dev/null)"
    printf 'worktree %s\n' "$repo"
    printf 'generated %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'patch_sha256 %s\n' "$(file_sha256 "$patch")"
    printf 'patch_bytes %s\n' "$(wc -c <"$patch" | tr -d ' ')"
    printf 'manifest_sha256 %s\n' "$(file_sha256 "$outdir/candidate.manifest")"
    printf 'path_set_sha256 %s\n' "$(manifest_paths_nul "$outdir/candidate.manifest" | stream_sha256)"
    printf 'entries %s\n' "$(grep -c . "$outdir/candidate.manifest")"
    printf 'patch_paths %s\n' "$(tr -cd '\0' <"$outdir/candidate.list.patch" | wc -c | tr -d ' ')"
    printf 'gitlinks %s\n' "$(grep -c . "$outdir/candidate.gitlinks" || true)"
    printf 'stable yes\n'
  } >"$meta" || die "cannot write $meta"

  note "base $base: $(grep -c . "$outdir/candidate.manifest") entries, stable across two passes"
  note "patch $patch ($(wc -c <"$patch" | tr -d ' ') bytes, sha256 $(file_sha256 "$patch"))"
  {
    echo "stage-dirty-candidate: on the host owning the canonical clone, into a fresh worktree:"
    echo "  git -C <canonical> worktree add <root>/<slug> $base"
    echo "  cd <root>/<slug> && git apply --binary --check <staged>/candidate.patch"
    echo "  cd <root>/<slug> && git apply --binary <staged>/candidate.patch"
    echo "  bash scripts/stage-dirty-candidate.sh verify <staged> <root>/<slug>"
  } >&2
}

cmd_verify() {
  [ $# -eq 2 ] || usage
  local outdir=$1 wt=$2 base got want actual mismatches=0 entries=0 mode sha blob path bad gitlink
  [ -f "$outdir/candidate.manifest" ] || die "no manifest at $outdir/candidate.manifest"
  base=$(awk '$1=="base"{print $2}' "$outdir/candidate.meta" 2>/dev/null)
  [ -n "$base" ] || die "no base recorded in $outdir/candidate.meta"
  got=$(git -C "$wt" rev-parse HEAD 2>/dev/null) || die "not a git work tree: $wt"
  [ "$got" = "$base" ] || die "worktree HEAD is $got, manifest base is $base"
  [ -z "$(git -C "$wt" diff --cached --name-only)" ] || die "worktree index has staged changes"

  if [ -f "$outdir/candidate.patch" ]; then
    patch_list "$outdir/candidate.patch" "$outdir/candidate.list.patch.v" || die "cannot read the patch"
    if ! cmp -s <(manifest_paths_nul "$outdir/candidate.manifest") "$outdir/candidate.list.patch.v"; then
      echo "MISMATCH the patch does not carry the same paths as the manifest" >&2
      diff <(manifest_paths_nul "$outdir/candidate.manifest" | tr '\0' '\n') \
        <(tr '\0' '\n' <"$outdir/candidate.list.patch.v") >&2
      mismatches=$((mismatches + 1))
    fi
  fi

  while read -r mode sha blob path; do
    [ -n "${path:-}" ] || continue
    entries=$((entries + 1))
    if [ "$mode" = DELETED ]; then
      [ -e "$wt/$path" ] && { echo "MISMATCH present but the manifest says deleted: $path" >&2; mismatches=$((mismatches + 1)); }
      continue
    fi
    if [ ! -f "$wt/$path" ]; then echo "MISMATCH missing: $path" >&2; mismatches=$((mismatches + 1)); continue; fi
    want=$sha
    actual=$(file_sha256 "$wt/$path")
    [ "$actual" = "$want" ] || { echo "MISMATCH content: $path" >&2; mismatches=$((mismatches + 1)); }
    [ "$(git -C "$wt" hash-object -- "$path" 2>/dev/null)" = "$blob" ] ||
      { echo "MISMATCH blob id: $path" >&2; mismatches=$((mismatches + 1)); }
    [ "$(file_mode "$wt/$path")" = "$mode" ] ||
      { echo "MISMATCH mode: $path (want $mode)" >&2; mismatches=$((mismatches + 1)); }
  done <"$outdir/candidate.manifest"

  # The same gitlink exclusion the manifest used. A recorded `candidate.gitlinks` list is honoured when
  # present; when it is absent (a candidate staged before gitlinks were handled) the list is empty and
  # the comparison behaves exactly as it did, so an older candidate is still verifiable.
  local -a excl=()
  if [ -f "$outdir/candidate.gitlinks" ]; then
    while IFS= read -r gitlink; do
      [ -n "$gitlink" ] || continue
      excl+=(":(exclude,literal)$gitlink")
      [ -z "$(git -C "$wt" diff --raw "$base" -- "$gitlink" 2>/dev/null)" ] ||
        { echo "MISMATCH submodule pointer moved: $gitlink" >&2; mismatches=$((mismatches + 1)); }
    done <"$outdir/candidate.gitlinks"
  fi

  disk_list "$wt" "$base" "$outdir/candidate.list.v" ${excl[@]+"${excl[@]}"} || die "cannot list the staged tree"
  if ! cmp -s <(manifest_paths_nul "$outdir/candidate.manifest") "$outdir/candidate.list.v"; then
    echo "MISMATCH dirty set: the staged tree holds paths the manifest does not name, or is missing some" >&2
    diff <(manifest_paths_nul "$outdir/candidate.manifest" | tr '\0' '\n') \
      <(tr '\0' '\n' <"$outdir/candidate.list.v") >&2
    mismatches=$((mismatches + 1))
  fi

  [ "$mismatches" -eq 0 ] || { note "FAILED: $mismatches mismatch(es) over $entries entries"; exit 1; }
  note "OK: $entries entries match byte-for-byte at base $base"
}

[ $# -ge 1 ] || usage
case "$1" in
  manifest) shift; cmd_manifest "$@" ;;
  verify) shift; cmd_verify "$@" ;;
  -h|--help|help) usage ;;
  *) usage ;;
esac
