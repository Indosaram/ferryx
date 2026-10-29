source .github/v41/common.sh
run build_cli timeout 3600 cargo build --manifest-path $M --no-default-features --bin ferryx-cli
mkdir -p q; B=src-tauri/target/debug/ferryx-cli; [ -x $B ] || B=target/debug/ferryx-cli; cp $B q/ferryx-cli
cp .github/v41/client.py .github/v41/incontainer.sh q/
docker pull -q debian:trixie
run qa docker run --rm -v $(pwd)/q:/q debian:trixie bash /q/incontainer.sh
cp q/*.txt ci-logs/ 2>/dev/null || true
