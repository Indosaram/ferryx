#!/bin/bash
# t44 gate (todo 44: the issued install command carries its own origin).
# Suites, 5 mutations, QA (self-hosted relay on loopback + fresh ubuntu:22.04 container), clippy.
set -u; mkdir -p ci-logs/qa
echo "HEAD=$(git rev-parse HEAD) TREE=$(git rev-parse HEAD^{tree})" | tee ci-logs/provenance.txt
run(){ n=$1; shift; echo "+ $*" | tee ci-logs/$n.log; "$@" 2>&1 | tee -a ci-logs/$n.log; rc=${PIPESTATUS[0]}; echo "EXIT_CODE=$rc" | tee -a ci-logs/$n.log; echo "$n exit=$rc" >> ci-logs/exits.txt; }
M=src-tauri/Cargo.toml
F=src-tauri/src/account/install_keys.rs

run build cargo test --manifest-path $M --lib --no-run
run suite-install_keys cargo test --manifest-path $M --lib -- account::install_keys --test-threads=4
run suite-account cargo test --manifest-path $M --lib -- account:: --test-threads=4
run suite-ssh-node-install cargo test --manifest-path $M --lib -- ipc::ssh::tests::node_install_command
run bun-install-test bun test scripts/install.test.mjs

mut(){ name=$1 file=$2 expr=$3; shift 3
  sed -i "$expr" "$file"
  if git diff --quiet -- "$file"; then echo "mut-$name NO-OP" >> ci-logs/exits.txt; return; fi
  git diff -- "$file" > ci-logs/mut-$name.diff
  for t in "$@"; do run "mut-$name" cargo test --manifest-path $M --lib -- "$t" --test-threads=4; done
  git checkout -- "$file"; }
# hosted shortcut removed -> hosted command gains a prefix
mut M1-hosted $F 's|if origin == crate::remote::state::DEFAULT_RELAY_URL {|if false {|' account::install_keys
# sh command loses FERRYX_ORIGIN
mut M2-sh-prefix $F 's#| FERRYX_ORIGIN={sh_origin} sh -s#| sh -s#' account::install_keys
# ps1 command loses $env:FERRYX_ORIGIN
mut M3-ps1-prefix $F 's#\$env:FERRYX_ORIGIN={ps1_origin}; ##' account::install_keys
# PowerShell quote escaping dropped
mut M4-ps1-escape $F 's#value\.replace(.*$#value)#' account::install_keys
# sh origin left unquoted
mut M5-sh-unquoted $F 's#let sh_origin = crate::ssh::direct::quote_posix(origin);#let sh_origin = origin.to_string();#' account::install_keys
git status --porcelain | tee ci-logs/restore-status.txt

run build-bins cargo build --manifest-path $M --no-default-features --bin ferryx-relay --bin ferryx-cli

cat > ci-logs/qa/qa.sh <<'QAEOF'
#!/bin/bash
# QA: self-hosted relay on 127.0.0.1:18787 (selfhost mode, FERRYX_DOWNLOADS_DIR holds this tree's
# ferryx-cli). The issued sh command is pasted into a fresh ubuntu:22.04 container (--network host)
# whose /etc/hosts sends relay.ferryx.dev to 127.0.0.9 (nothing listens there; every attempt is a
# packet on lo). github.com -> a local HTTPS stub that serves only SHA256SUMS for this tree's CLI,
# because install.sh verifies against the GitHub release's SHA256SUMS and this build is unreleased.
set -u
W=$PWD; Q=$W/ci-logs/qa; T=$W/src-tauri/target/debug; CLI=$T/ferryx-cli
O=http://127.0.0.1:18787
command -v tcpdump >/dev/null || sudo apt-get install -y -qq tcpdump >/dev/null
glibc_of(){ objdump -T "$1" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1; }
need=$(glibc_of $CLI); echo "host-built ferryx-cli needs GLIBC_$need (ubuntu:22.04 has 2.35)"
if [ "$(printf '%s\n' "$need" 2.35 | sort -V | tail -1)" != 2.35 ]; then
  echo "rebuilding ferryx-cli inside ubuntu:22.04"
  ZIGDIR=$(dirname "$(readlink -f "$(command -v zig)")")
  docker run --rm -v "$W":/w -v "$HOME/.cargo":/c -v "$HOME/.rustup":/r -v "$ZIGDIR":/zig \
    -e CARGO_HOME=/c -e RUSTUP_HOME=/r -e PATH=/c/bin:/zig:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -e CARGO_TARGET_DIR=/w/target-2204 -e ZIG_GLOBAL_CACHE_DIR=/tmp/zc -e CMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -e CARGO_PROFILE_DEV_DEBUG=0 ubuntu:22.04 bash -c 'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config git >/dev/null && git config --global --add safe.directory "*" && cd /w && cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli' \
    > $Q/cli-2204-build.log 2>&1 || { echo "ubuntu:22.04 build FAILED"; tail -60 $Q/cli-2204-build.log; exit 1; }
  CLI=$W/target-2204/debug/ferryx-cli; echo "2204-built ferryx-cli needs GLIBC_$(glibc_of $CLI)"
fi

D=$Q/relay; mkdir -p $D/home $D/data $D/dl
cp $CLI $D/dl/ferryx-cli-linux-amd64
SHA=$(sha256sum $D/dl/ferryx-cli-linux-amd64 | cut -d' ' -f1); echo "served ferryx-cli sha256=$SHA"

# GitHub SHA256SUMS stub (TLS, CA trusted only inside the QA container).
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=t44-qa-ca -keyout $Q/ca.key -out $Q/ca.crt \
  -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj /CN=github.com -keyout $Q/gh.key -out $Q/gh.csr 2>/dev/null
printf 'subjectAltName=DNS:github.com\n' > $Q/gh.ext
openssl x509 -req -in $Q/gh.csr -CA $Q/ca.crt -CAkey $Q/ca.key -CAcreateserial -days 1 -extfile $Q/gh.ext -out $Q/gh.crt 2>/dev/null
mkdir -p $Q/gh/Indosaram/ferryx/releases/latest/download
echo "$SHA  ferryx-cli-linux-amd64" > $Q/gh/Indosaram/ferryx/releases/latest/download/SHA256SUMS
sudo python3 -c 'import http.server,ssl,os,sys
os.chdir(sys.argv[1]); s=http.server.ThreadingHTTPServer(("127.0.0.1",443),http.server.SimpleHTTPRequestHandler)
c=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); c.load_cert_chain(sys.argv[2],sys.argv[3]); s.socket=c.wrap_socket(s.socket,server_side=True); s.serve_forever()' \
  $Q/gh $Q/gh.crt $Q/gh.key > $Q/gh-stub.log 2>&1 &
GHP=$!

env HOME=$D/home FERRYX_DEPLOYMENT_MODE=selfhost FERRYX_ACCOUNT_ORIGIN=$O FERRYX_ACCOUNT_DATA_DIR=$D/data \
  FERRYX_MAIL_DEV_FILE=1 FERRYX_DOWNLOADS_DIR=$D/dl RUST_LOG=info \
  $T/ferryx-relay --bind 127.0.0.1 --port 18787 > $D/relay.log 2>&1 &
RP=$!
ok=0; for i in $(seq 120); do curl -sf $O/api/account/v1/health >/dev/null && { ok=1; break; }; sleep 0.5; done
echo "relay pid=$RP health=$ok"; [ $ok = 1 ] || { cat $D/relay.log; exit 1; }

curl -s -X POST $O/api/account/v1/login/request -H 'content-type: application/json' -d '{"email":"qa@example.com"}'; echo
LINK=$(cat $D/data/mail/*.txt | head -1); CODE=${LINK##*code=}
TOK=$(curl -s -X POST $O/api/account/v1/login/consume -H 'content-type: application/json' -d "{\"code\":\"$CODE\"}" | jq -r .token)
echo "session token len=${#TOK}"
issue(){ curl -s -X POST $O/api/account/v1/install-keys -H "authorization: Bearer $TOK" -H 'content-type: application/json' -d "{\"label\":\"$1\"}"; }
issue t44-new > $Q/issued-new.json; issue t44-control > $Q/issued-control.json
CMD=$(jq -r .installCommand.sh $Q/issued-new.json); PS1=$(jq -r .installCommand.ps1 $Q/issued-new.json)
KEY2=$(jq -r .key $Q/issued-control.json)
echo "ISSUED_SH=$CMD"; echo "ISSUED_PS1=$PS1"
# The control is today's (pre-todo-44) command for the same origin, from the base format string.
CTRL="curl -fsSL $O/install.sh | sh -s -- --key $KEY2"; echo "CONTROL_SH=$CTRL"

docker build -q -t t44-ub2204 - > /dev/null <<'DF'
FROM ubuntu:22.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends curl ca-certificates && rm -rf /var/lib/apt/lists/*
DF
docker run --rm ubuntu:22.04 ldd --version | head -1

cap_start(){ sudo tcpdump -i lo -nn -U -s0 -w $Q/$1-lo.pcap 'tcp port 18787 or host 127.0.0.9' > $Q/$1-lo.err 2>&1 & LO=$!
  sudo tcpdump -i any -nn -U -s0 -w $Q/$1-dns.pcap 'port 53' > $Q/$1-dns.err 2>&1 & DN=$!
  for i in $(seq 40); do grep -q listening $Q/$1-lo.err && grep -q listening $Q/$1-dns.err && break; sleep 0.25; done; }
cap_stop(){ sudo kill -INT $LO $DN; wait $LO $DN 2>/dev/null; }
paste_in_container(){ docker run --rm --network host --add-host relay.ferryx.dev:127.0.0.9 --add-host github.com:127.0.0.1 \
  -v $Q/ca.crt:/usr/local/share/ca-certificates/t44-qa.crt:ro -e CMD="$1" t44-ub2204 sh -c '
    update-ca-certificates >/dev/null 2>&1
    echo "--- container /etc/hosts:"; grep -E "ferryx|github" /etc/hosts; echo "--- getent: $(getent hosts relay.ferryx.dev)"
    echo "--- pasting: $CMD"; sh -c "$CMD"; rc=$?; echo "INSTALL_EXIT=$rc"
    echo "--- enrollment record:"; cat /root/.ferryx/remote/account-enrollment.json 2>/dev/null || find / -name "account-enrollment.json" -not -path "/proc/*" -exec cat {} \; 2>/dev/null; echo; exit $rc'; }
report(){ p=$1
  sudo tcpdump -r $Q/$p-lo.pcap -nn -A 'tcp dst port 18787' 2>/dev/null | grep -aoE '(GET|POST) /[^ ]* HTTP/1\.[01]' > $Q/$p-access.log
  echo "--- $p self-hosted relay access log (from lo capture):"; cat $Q/$p-access.log
  N9=$(sudo tcpdump -r $Q/$p-lo.pcap -nn 'host 127.0.0.9' 2>/dev/null | wc -l)
  NDNS=$(sudo tcpdump -r $Q/$p-dns.pcap -nn 2>/dev/null | grep -c 'relay\.ferryx\.dev')
  echo "$p packets to relay.ferryx.dev (127.0.0.9)=$N9; DNS queries for relay.ferryx.dev=$NDNS"; }
has(){ grep -qxF "$2 HTTP/1.1" $Q/$1-access.log; }

echo "##### NEW: issued command"
cap_start new; paste_in_container "$CMD" 2>&1 | tee $Q/new-container.log; cap_stop; report new
NEW_EXIT=$(grep -o 'INSTALL_EXIT=[0-9]*' $Q/new-container.log | cut -d= -f2)
MACH=$(curl -s $O/api/account/v1/machines -H "authorization: Bearer $TOK"); echo "machines after NEW: $MACH"
NMACH=$(echo "$MACH" | jq 'if type=="array" then length else (.machines|length) end')
NEW_N9=$N9; NEW_DNS=$NDNS

echo "##### CONTROL: today's command for the same origin"
cap_start ctl; paste_in_container "$CTRL" 2>&1 | tee $Q/ctl-container.log; cap_stop; report ctl
CTL_EXIT=$(grep -o 'INSTALL_EXIT=[0-9]*' $Q/ctl-container.log | cut -d= -f2)
echo "--- github stub log:"; cat $Q/gh-stub.log

kill $RP; wait $RP 2>/dev/null; sudo kill $GHP 2>/dev/null; sudo pkill -f 'ThreadingHTTPServer' 2>/dev/null
v=PASS; chk(){ if eval "$2"; then echo "CHECK PASS $1"; else echo "CHECK FAIL $1"; v=FAIL; fi; }
chk new-command-carries-origin "echo \"\$CMD\" | grep -qF \"FERRYX_ORIGIN='$O'\""
chk new-install-exit-0 "[ \"$NEW_EXIT\" = 0 ]"
chk new-get-install-sh "has new 'GET /install.sh'"
chk new-download-from-selfhost "has new 'GET /download/ferryx-cli-linux-amd64'"
chk new-enroll-challenge "has new 'POST /api/account/v1/machines/enroll/challenge'"
chk new-enroll "has new 'POST /api/account/v1/machines/enroll'"
chk new-machine-enrolled "[ \"$NMACH\" = 1 ]"
chk new-no-packet-to-relay.ferryx.dev "[ $NEW_N9 = 0 ]"
chk new-no-dns-for-relay.ferryx.dev "[ $NEW_DNS = 0 ]"
chk control-detector-sees-relay.ferryx.dev "[ $N9 -gt 0 ]"
chk control-no-selfhost-download "! has ctl 'GET /download/ferryx-cli-linux-amd64'"
chk control-no-enroll "! has ctl 'POST /api/account/v1/machines/enroll'"
chk control-install-fails "[ \"$CTL_EXIT\" != 0 ]"
echo "QA_RESULT=$v"; [ $v = PASS ]
QAEOF
run qa bash ci-logs/qa/qa.sh
sudo chown -R "$(id -u)" ci-logs || true
rm -f ci-logs/qa/*.key

run clippy cargo clippy --manifest-path $M --all-targets --message-format=short
grep -E "^(src|tests|examples|benches)/" ci-logs/clippy.log | sed -E 's/:[0-9]+:[0-9]+:/:/' | sort | uniq -c > ci-logs/clippy.n
grep -E "install_keys\.rs|onboarding_tests\.rs" ci-logs/clippy.log > ci-logs/clippy-own.txt; echo "own-file clippy lines: $(wc -l < ci-logs/clippy-own.txt)" | tee -a ci-logs/exits.txt
grep -hE "^test .* FAILED$|^test result" ci-logs/*.log > ci-logs/summary.txt
echo GATE-DONE
