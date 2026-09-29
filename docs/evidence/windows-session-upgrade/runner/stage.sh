#!/usr/bin/env bash
# Freeze a phase1 run: parent bundle, delta tar + manifest, tested tree sha, runner copies.
# Reads the checkout through git only; objects go to a temp object dir (shared .git untouched).
set -euo pipefail

W=/Volumes/T9-Mac/project/ferryx-windows-session-upgrade
EVID_REL=docs/evidence/windows-session-upgrade
FEATURE_BASE=38276c9c584221122efc2274852e457c080a449b

NN=${1:-}
[[ $NN =~ ^[0-9]{2}$ ]] || { echo "usage: stage.sh NN (two-digit attempt number)" >&2; exit 64; }

cd "$W"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 LC_ALL=C COPYFILE_DISABLE=1
PARENT=$(git rev-parse HEAD)
[[ $PARENT == "$FEATURE_BASE" ]] || { echo "PARENT_NOT_PHASE1_BASE: HEAD=$PARENT" >&2; exit 65; }

mkdir -p /tmp/ferryx-wsu
TMP=$(mktemp -d /tmp/ferryx-wsu/stage.XXXXXX)
trap 'rm -rf "$TMP"' EXIT
COMMON=$(git rev-parse --path-format=absolute --git-common-dir)
export GIT_OBJECT_DIRECTORY=$TMP/objects GIT_ALTERNATE_OBJECT_DIRECTORIES=$COMMON/objects
mkdir -p "$TMP/objects" "$TMP/delta-root"

git --no-optional-locks -c core.quotepath=false status --porcelain=v1 -z \
  --untracked-files=all --no-renames --ignore-submodules=all > "$TMP/status.z"

MAN=$TMP/manifest.tsv
: > "$MAN"
while IFS= read -r -d '' rec; do
  path=${rec:3}
  case $path in "$EVID_REL"/*) continue ;; esac
  case $path in *$'\t'* | *$'\n'*) echo "UNSUPPORTED_PATH_CHARS: $path" >&2; exit 65 ;; esac
  if [[ -L $path ]]; then echo "UNSUPPORTED_SYMLINK: $path" >&2; exit 65; fi
  if [[ -d $path ]]; then echo "UNSUPPORTED_DIR_ENTRY: $path" >&2; exit 65; fi
  if [[ -f $path ]]; then
    # Snapshot first so hashes and tar content come from the same bytes.
    mkdir -p "$TMP/delta-root/$(dirname "$path")"
    cp -p "$path" "$TMP/delta-root/$path"
    f=$TMP/delta-root/$path
    if [[ -x $f ]]; then mode=100755; else mode=100644; fi
    blob=$(git hash-object -w --path="$path" "$f")
    sha=$(shasum -a 256 "$f" | cut -d' ' -f1)
    bytes=$(stat -f %z "$f")
    printf 'P\t%s\t%s\t%s\t%s\t%s\n' "$mode" "$blob" "$sha" "$bytes" "$path" >> "$MAN"
  else
    printf 'D\t-\t-\t-\t-\t%s\n' "$path" >> "$MAN"
  fi
done < "$TMP/status.z"
sort -t $'\t' -k6,6 "$MAN" -o "$MAN"

export GIT_INDEX_FILE=$TMP/index
git read-tree "$PARENT"
while IFS=$'\t' read -r st mode blob _sha _bytes path; do
  if [[ $st == P ]]; then
    git update-index --add --cacheinfo "$mode,$blob,$path"
  else
    git update-index --force-remove -- "$path"
  fi
done < "$MAN"
TREE=$(git write-tree)
unset GIT_INDEX_FILE

DELTA_SHA=$(shasum -a 256 "$MAN" | cut -d' ' -f1)
RUN=wsu-${PARENT:0:8}-${DELTA_SHA:0:8}-r$NN
STAGE=/tmp/ferryx-wsu/$RUN
OUT=$W/$EVID_REL/$RUN
[[ ! -e $STAGE && ! -e $OUT ]] || { echo "RUN_EXISTS: $RUN" >&2; exit 66; }

IN=$STAGE/remote/in
mkdir -p "$IN/runner"
cp "$MAN" "$IN/manifest.tsv"
tar --no-mac-metadata -cf "$IN/delta.tar" -C "$TMP/delta-root" .
git bundle create "$IN/parent.bundle" HEAD
git bundle list-heads "$IN/parent.bundle" | grep -qx "$PARENT HEAD" \
  || { echo "BUNDLE_HEAD_MISMATCH (HEAD moved during stage)" >&2; exit 65; }
cp "$W/$EVID_REL/runner/"*.ps1 "$IN/runner/"

BUNDLE_SHA=$(shasum -a 256 "$IN/parent.bundle" | cut -d' ' -f1)
TAR_SHA=$(shasum -a 256 "$IN/delta.tar" | cut -d' ' -f1)
cat > "$IN/run.json" <<EOF
{
  "run": "$RUN",
  "phase": "phase1-foundation",
  "parentSha": "$PARENT",
  "featureBaseSha": "$FEATURE_BASE",
  "deltaManifestSha256": "$DELTA_SHA",
  "testedTreeSha": "$TREE",
  "bundleSha256": "$BUNDLE_SHA",
  "deltaTarSha256": "$TAR_SHA",
  "remoteRoot": "C:\\\\Users\\\\sook\\\\ferryx-wsu\\\\$RUN",
  "stagedUtc": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

(cd "$IN" && find . -type f ! -name in-manifest.tsv -print | sed 's|^\./||' | sort |
  while IFS= read -r f; do printf '%s\t%s\n' "$(shasum -a 256 "$f" | cut -d' ' -f1)" "$f"; done) > "$IN/in-manifest.tsv"

[[ $(git rev-parse HEAD) == "$PARENT" ]] || { echo "HEAD_MOVED_DURING_STAGE" >&2; exit 65; }

CONTRACT=present
grep -q $'\tsrc-tauri/tests/windows_session_host_contract.rs$' "$MAN" || CONTRACT=absent
mkdir -p "$OUT"
cp "$IN/run.json" "$IN/manifest.tsv" "$IN/in-manifest.tsv" "$OUT/"
cat > "$OUT/frozen-paths.txt" <<EOF
run=$RUN
checkout=$W
localStage=$STAGE/remote/in
remoteRoot=C:\\Users\\sook\\ferryx-wsu\\$RUN
ghosttyJunction=src\\src-tauri\\vendor\\ghostty -> C:\\Users\\sook\\ferryx-ghostty @ 6a508fd5e34c7e222c052a6d00bb3891ff3feace
contractFixture=src-tauri/tests/windows_session_host_contract.rs ($CONTRACT in delta)
steps=preflight checkout ui-install ui-build build-bin lib-session-host lib-output-hub contract lib-full cleanup
EOF
echo "RUN=$RUN contract=$CONTRACT"
