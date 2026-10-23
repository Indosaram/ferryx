# 머신 온보딩 · 워크스페이스 디스커버리 전수조사 (2026-10-23)

## 요약

- **결론:** SSH가 되는 머신은 데스크탑이 *단일 사용 설치 키*를 발급해 SSH stdin으로 넘기고, 원격에서 CLI 설치, 계정 등록, 데몬 서비스 등록까지 한 번에 끝냅니다. SSH가 안 되는 머신은 같은 설치 키를 넣은 원라이너를 쓰거나, 키 없이 실행한 CLI가 띄우는 승인 코드를 소유자가 앱이나 웹에서 승인합니다. 이메일 매직 링크는 폴백으로 남깁니다.
- **업계 표준과의 비교:** 단일 사용·단기 TTL·해시 저장 키는 Tailscale, NetBird, kubeadm, GitHub Actions, Buildkite와 같은 모델입니다. 승인 화면에 기기 정보를 보여주고 거절을 기본값으로 두는 것은 RFC 8628, Tailscale device approval, Syncthing과 같은 모델입니다.
- **피해야 할 패턴:** 재사용·무기한 토큰(cloudflared, ngrok, Portainer `EDGE_KEY`), 기기 정보 없는 맹목 승인(Coder, Chrome Remote Desktop), argv에 시크릿을 넣는 방식은 피합니다.
- **디스커버리:** Orca, Crystal, Superset, VS Code처럼 데몬 쪽에서 범위를 제한해 스캔합니다(깊이, 시간, 개수 상한; `.git`에서 하강 중단; 심볼릭 링크 미추적). 결과는 `git worktree` 기준으로 묶습니다. 다른 IDE의 최근 목록을 읽는 기능은 보류합니다.
- **서비스 등록:** systemd user + linger, launchd, Windows 예약 작업을 쓰고, systemd가 없는 컨테이너형 호스트에서는 분리 실행으로 폴백합니다.

## 1. 조사 범위와 방법

- **조사 축:** 48개 축을 두 런으로 나눠 조사했습니다.
  - 등록·보안: h00–h23
  - 경쟁 앱·원격 설치: h24–h35
  - 디스커버리·서비스 등록: h36–h47
- **코드베이스:** Ferryx 코드베이스는 4개 레인(c1 등록, c2 SSH, c3 디스커버리, c4 서비스)으로 따로 조사했습니다.
- **검증:** 모든 원천 파일은 사실 표, 반론 점검(Counter-check), 출처 URL로 구성됩니다. 축약 단계에서는 원천에 실제로 있는 URL만 옮겼고, 이 문서의 URL은 원천 파일과 문자 단위로 대조해 모두 일치함을 확인했습니다.
- **원천 위치:** `.omo/ulw-research/20261023-105211-machine-onboarding/` (waves, reduce, report).

## 2. 제품별 매트릭스

## 2.1 등록·보안 토큰

| 제품 | 분류 | 방식(명령/TTL/기본값) | 승인·보안 | 출처 |
|---|---|---|---|---|
| Tailscale | Mesh VPN | `curl -fsSL https://tailscale.com/install.sh \| sh`로 설치한 뒤 `tailscale up`을 실행합니다. 키 없이 실행하면 `https://login.tailscale.com/a/<hex>` 링크를 출력합니다. 플래그는 `--qr`, `--json`(`AuthURL`/`BackendState`), `--auth-key=file:`, `--timeout`(기본값 `0s`)이 있습니다. auth key는 일회용과 재사용 중에서 고를 수 있고, TTL은 1–90일(기본값 90일)입니다. 키 옵션으로 ephemeral, pre-approved, tags를 붙일 수 있습니다. auth key는 저장되지 않으며, 상태는 `/var/lib/tailscale/tailscaled.state`에 둡니다(디렉터리 권한 0700). 컨테이너에서는 `TS_AUTH_ONCE`를 씁니다. 설치 스크립트는 `main()`으로 감싸져 있습니다. | Device approval을 켜면 "Needs approval" 배지가 붙습니다. `nodeNeedsApproval` 웹훅과 `POST /api/v2/device/{id}/authorized`로 승인을 자동화할 수 있습니다. 키를 폐기해도 이미 등록된 노드는 해제되지 않습니다. node key는 기본 180일 뒤 만료되고, tagged 기기는 만료되지 않습니다. Tailnet Lock과 EncryptState를 지원하며, MDM AuthKey는 Danger로 표시됩니다. | https://tailscale.com/kb/1031/install-linux ; https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/up.go ; https://tailscale.com/kb/1241/tailscale-up ; https://tailscale.com/kb/1085/auth-keys ; https://tailscale.com/kb/1099/device-approval ; https://tailscale.com/kb/1028/key-expiry ; https://github.com/tailscale/tailscale/blob/main/types/persist/persist.go ; https://github.com/tailscale/tailscale/blob/main/scripts/installer.sh ; https://github.com/tailscale/tailscale/blob/main/cmd/containerboot/main.go ; https://tailscale.com/kb/1226/tailnet-lock ; https://raw.githubusercontent.com/tailscale/terraform-provider-tailscale/main/docs/resources/tailnet_key.md ; https://tailscale.com/docs/features/access-control/auth-keys.md ; https://tailscale.com/docs/features/tailscale-system-policies.md ; https://tailscale.com/install.sh |
| Headscale | Tailscale 셀프호스팅 컨트롤 플레인 | `headscale preauthkeys create`로 만든 키는 기본적으로 일회용이고 1시간 뒤 만료됩니다. `--reusable`, `--ephemeral`를 붙일 수 있습니다. 키는 `hskey-auth-` 접두사를 쓰고, 접두사와 해시만 저장됩니다. 웹 등록은 `headscale auth register --auth-id`로 처리합니다. | 웹 등록은 관리자가 CLI로 승인합니다. | https://github.com/juanfont/headscale/blob/main/cmd/headscale/cli/preauthkeys.go ; https://github.com/juanfont/headscale/blob/main/hscontrol/types/preauth_key.go ; https://headscale.net/stable/ref/registration/ ; https://github.com/juanfont/headscale/blob/main/CHANGELOG.md |
| NetBird | Mesh VPN | 명령 2개로 설치하며, 설치 스크립트는 `curl --proto =https`를 씁니다. setup key는 일회용과 재사용 중에서 고를 수 있고, 재사용 키의 기본 사용 횟수는 무제한입니다. "Add Peer"는 24시간짜리 일회용 키를 만들고, 키 생성 대화상자의 기본값은 7일입니다. 서버 기본 키는 재사용 가능하며 30일짜리입니다. 키는 `sha256` 해시로 저장되고 5자 마스크로만 표시됩니다. | peer 승인 토글이 있습니다. 키를 폐기해도 등록된 peer는 남습니다. 문서에 "최종 사용자 기기 등록에 setup key를 쓰지 말라"고 적혀 있습니다. | https://docs.netbird.io/how-to/register-machines-using-setup-keys ; https://github.com/netbirdio/netbird/blob/main/management/server/types/setupkey.go ; https://docs.netbird.io/manage/peers/bootstrap-via-config-file ; https://docs.netbird.io/how-to/approve-peers ; https://docs.netbird.io/how-to/getting-started ; https://pkgs.netbird.io/install.sh ; https://raw.githubusercontent.com/netbirdio/docs/ff6170673ef10e3e1adfa5aca80a51758bc76201/src/pages/manage/peers/register-machines-using-setup-keys.mdx |
| ZeroTier | Mesh VPN | `zerotier-cli join <nwid>`로 참여하며 join 토큰이 없습니다. 로컬 API 시크릿은 `authtoken.secret`입니다. | 관리자가 "Auth?"를 체크하거나 API로 `{"authorized":true}`를 보낼 때까지 노드는 `ACCESS_DENIED` 상태입니다. 승인은 전적으로 관리자가 합니다. | https://docs.zerotier.com/cli/ ; https://docs.zerotier.com/start/ ; https://docs.zerotier.com/controller/ ; https://docs.zerotier.com/tokens/ |
| Teleport | 접근 프록시 | `tctl tokens add --type=node --ttl`의 기본 TTL은 30분입니다(`ProvisioningTokenTTL`). 토큰은 랜덤 16바이트를 hex로 표현합니다. Web UI에서 만든 토큰은 4시간짜리입니다(`NodeJoinTokenTTL`). `/scripts/:token/install-node.sh`는 서버에서 토큰을 검증하고 CA pin을 스크립트에 넣습니다. 노드 설정은 `teleport node configure --token=<file>`로 하며, `bound_keypair` 조인 방식도 있습니다. | 토큰은 TTL이 끝날 때까지 재사용할 수 있고, bot 토큰만 일회용입니다. 노드별 승인 대기열은 없습니다. Headless 승인 링크(`/web/headless/<uuid>`)는 MFA를 요구합니다. static 토큰은 권장하지 않습니다. | https://goteleport.com/docs/reference/deployment/join-methods/ ; https://raw.githubusercontent.com/gravitational/teleport/master/tool/tctl/common/token_command.go ; https://raw.githubusercontent.com/gravitational/teleport/master/lib/defaults/defaults.go ; https://goteleport.com/docs/installation/agents/join-token/ ; https://raw.githubusercontent.com/gravitational/teleport/master/lib/auth/join.go ; https://raw.githubusercontent.com/gravitational/teleport/master/lib/web/join_tokens.go ; https://raw.githubusercontent.com/gravitational/teleport/master/lib/web/scripts/node-join/install.sh ; https://goteleport.com/docs/zero-trust-access/authentication/headless/ ; https://github.com/gravitational/teleport/discussions/47620 ; https://goteleport.com/docs/enroll-resources/auto-discovery/servers/ec2-discovery/ ; https://github.com/gravitational/teleport/issues/11137 |
| cloudflared | 터널 | `cloudflared service install <token>`로 설치합니다. 토큰은 base64로 인코딩한 JSON `{a,s,t}`이고, 만료되지 않으며, 최대 25개 replica가 재사용할 수 있습니다. 토큰은 `/etc/cloudflared/token`(0600)에 저장되고 `--token-file`로 전달됩니다. 이는 argv 노출 취약점(VULN-118896, 2026.7.x)의 수정입니다. `tunnel login`은 long-poll 후 `cert.pem`을 받는데, 계정 전체 권한에 유효기간 10년입니다. 자동 업데이트 타이머도 설치됩니다. | 승인 단계가 없습니다. 토큰을 회전해도 기존 connector는 유지되므로, 끊으려면 연결을 `DELETE`해야 합니다. | https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel/index.md ; https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel-api/index.md ; https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/linux_service.go ; https://github.com/cloudflare/cloudflared/commit/f70adda11cf4c1acf1aa9c1df952baa05269f074 ; https://github.com/cloudflare/cloudflared/issues/1539 ; https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/remote-tunnel-permissions/index.md ; https://developers.cloudflare.com/tunnel/reference/tunnel-tokens/index.md ; https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/tunnel/login.go ; https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/tunnel-permissions/index.md |
| ngrok | 터널 | `ngrok config add-authtoken <token>`으로 등록합니다. 토큰은 재사용할 수 있고 TTL이 없으며, 한 번만 표시됩니다. `bind` ACL로 범위를 제한할 수 있습니다. `ngrok service install --config`는 관리자 권한이 필요하고, root 유닛이 사용자 홈의 설정 파일을 읽습니다. 세션 목록은 `GET /tunnel_sessions`로 조회하며, Service Users를 지원합니다. | 승인 단계가 없습니다. | https://ngrok.com/docs/gateway/agent/authtokens.md ; https://ngrok.com/docs/api-reference/credentials/create.md ; https://ngrok.com/docs/gateway/agent/config/v3.md ; https://ngrok.com/docs/gateway/agent/service.md ; https://github.com/inconshreveable/ngrok/issues/839 ; https://ngrok.com/docs/pricing-limits/free-plan-limits.md ; https://ngrok.com/docs/api-reference/tunnelsessions/list.md ; https://ngrok.com/docs/iam/service-users.md ; https://ngrok.com/docs/gateway/device-gateway/fleet-management.md |
| Portainer Edge Agent | 컨테이너 관리 | `EDGE_KEY`는 `url\|tunnel\|fingerprint\|endpoint_id`를 base64로 인코딩한 값이고, TTL 없이 재사용됩니다. `/data/agent_edge_key`(0644)에 저장됩니다. 키를 붙여넣는 UI가 `:80`에서 15분 동안 열립니다. 터널 자격증명은 5분 동안 유휴 상태면 만료됩니다. `EnforceEdgeID`의 기본값은 true입니다. | 에이전트가 처음 poll할 때 Edge ID를 TOFU(최초 접속 신뢰)로 받아들입니다. 새 기기는 waiting room에 들어가고 "Associate Device"로 승인합니다. `TrustOnFirstConnect`와 mTLS는 선택 사항입니다. | https://docs.portainer.io/admin/environments/add/docker/edge ; https://github.com/portainer/agent/blob/develop/edge/key.go ; https://github.com/portainer/agent/blob/develop/README.md ; https://github.com/portainer/portainer/blob/develop/api/http/security/bouncer.go ; https://docs.portainer.io/user/edge/waiting-room.md ; https://docs.portainer.io/advanced/edge-agent ; https://github.com/portainer/portainer/issues/5473 ; https://www.portainer.io/how-to/how-to-secure-the-portainer-edge-agent-comms-with-mtls |
| Netdata | 모니터링 | `kickstart.sh --claim-token --claim-rooms --claim-url`로 설치하며, `claim.conf`(0640 root:netdata)를 씁니다. 에이전트가 RSA-2048 키를 만들고 `PUT /api/v1/spaces/nodes/{id}`로 등록합니다. 종료 코드가 정해져 있어 12는 만료, 13은 복제된 노드를 뜻합니다. | 승인 단계가 없습니다. Space 토큰은 만료되지 않고 재사용할 수 있으며, 재발급할 수 있습니다. 로컬 `netdata_random_session_id`는 일회용이고 시도할 때마다 바뀝니다. | https://raw.githubusercontent.com/netdata/netdata/master/packaging/installer/methods/kickstart.md ; https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/packaging/installer/kickstart.sh#L1611-L1628 ; https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/src/claim/claim-with-api.c#L198-L337 ; https://community.netdata.cloud/t/is-the-token-for-claiming-new-nodes-different-each-time/1026 ; https://raw.githubusercontent.com/netdata/netdata/master/src/claim/README.md ; https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/src/web/api/v2/api_v2_claim.c#L144-L272 ; https://raw.githubusercontent.com/netdata/netdata/master/docs/learn/node-identities.md ; https://community.netdata.cloud/t/automate-netdata-cloud-node-claim-on-instance-deploy/25 |
| Datadog Agent | 모니터링 | `DD_API_KEY=... bash -c "$(curl -L https://install.datadoghq.com/scripts/install_script_agent7.sh)"`로 설치합니다. 조직 API key를 쓰는데 만료가 없습니다. 키는 `datadog.yaml`(640)에 저장됩니다. fleet 설치 명령 생성기는 `?platform=`을 받습니다. | 승인 단계가 없습니다. 텔레메트리에 API key가 포함되어 전송됩니다. | https://github.com/DataDog/documentation/blob/9f8f2bea7aceed52e9b44d1c2d8cdacb81bb2213/hugo/content/en/agent/guide/upgrade.md ; https://github.com/DataDog/agent-linux-install-script/blob/973102c6145c00744d08f4f1f37af5391d08746f/install_script.sh.template ; https://docs.datadoghq.com/account_management/api-app-keys.md ; https://docs.datadoghq.com/agent/supported_platforms/osx.md ; https://docs.datadoghq.com/agent/supported_platforms/windows.md ; https://docs.datadoghq.com/agent/fleet_automation.md |
| New Relic CLI | 모니터링 | `newrelic install`은 호스트에서 `NRAK` 사용자 키로 실행되며 license key를 가져옵니다. | 사용자 범위 키가 호스트에 남습니다. 승인 단계가 없습니다. | https://github.com/newrelic/newrelic-cli/blob/4c62d7ab4133f0b082bba2ad10daca6fa7211570/internal/install/command.go ; https://raw.githubusercontent.com/newrelic/docs-website/develop/src/content/docs/apis/intro-apis/new-relic-api-keys.mdx ; https://raw.githubusercontent.com/newrelic/docs-website/develop/src/content/docs/opentelemetry/db360/mysql/cli.mdx |
| Grafana Alloy / gcx | 모니터링 | `install-linux.sh`는 `GCLOUD_RW_API_KEY`를 받고 SHA-256을 검증합니다. 시크릿은 systemd drop-in `env.conf`(600)에 저장됩니다. macOS에서는 실행을 거부합니다. 권한 범위는 `set:alloy-data-write`이고, 만료일을 지정하지 않으면 토큰은 만료되지 않습니다. `gcx login`은 검증 코드를 표시합니다. | 범위가 제한된 access-policy 토큰입니다. 승인 단계가 없습니다. | https://storage.googleapis.com/cloud-onboarding/alloy/scripts/install-linux.sh ; https://grafana.com/docs/grafana-cloud/send-data/fleet-management/set-up/onboard-collectors/standalone-alloy/index.md ; https://grafana.com/docs/grafana-cloud/security-and-account-management/authentication-and-permissions/access-policies/create-access-policies/index.md ; https://github.com/grafana/gcx/blob/1a040db2557a7b0a4cf9cf1a9d762d4f1b58cd30/docs/reference/login.md |
| GitHub Actions self-hosted runner | CI runner | 등록 토큰은 1시간짜리이고 `config.sh --url --token`으로 등록합니다. runner가 RSA-2048 키를 만들어 공개키만 업로드하고, 개인키는 `.credentials_rsaparams`(chmod 600)에 둡니다. 제거에는 별도 토큰이 필요합니다. JIT config는 서버가 자격증명을 넣어 둔 채로 job 하나만 실행합니다. `svc.sh`는 systemd에서는 sudo가 필요하고, macOS에서는 sudo 없이 LaunchAgent로 설치해야 합니다. `--replace`를 지원합니다. | 승인 단계가 없습니다. 오프라인 runner는 14일 뒤 자동 삭제되고, 업데이트에는 30일 유예가 있습니다. 토큰이 일회용인지는 검증하지 못했습니다. | https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners ; https://docs.github.com/en/rest/actions/self-hosted-runners ; https://docs.github.com/en/actions/reference/security/secure-use ; https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Listener/Configuration/ConfigurationManager.cs ; https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Misc/layoutbin/darwin.svc.sh.template ; https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Misc/layoutbin/systemd.svc.sh.template ; https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/remove-runners ; https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/docs/automate.md ; https://docs.github.com/en/actions/reference/runners/self-hosted-runners |
| GitLab Runner | CI runner | `gitlab-runner register --token glrt-...`로 등록합니다. 토큰은 UI에 잠깐만 표시되고 `config.toml`에 저장됩니다. 토큰 하나로 여러 manager를 붙일 수 있으며, `system_id`(`s_`/`r_`)로 구분합니다. `POST /user/runners`에는 `create_runner` scope가 필요합니다. | 구형 registration token은 17.0부터 `410 Gone`을 반환하고 20.0에서 제거됩니다. 공유 토큰에는 회전 관련 버그가 있습니다. runner는 7일이 지나면 stale로 표시됩니다. | https://docs.gitlab.com/ci/runners/new_creation_workflow/ ; https://docs.gitlab.com/runner/register/ ; https://docs.gitlab.com/security/tokens/ ; https://docs.gitlab.com/ci/runners/runners_scope/ ; https://docs.gitlab.com/api/users/ ; https://docs.gitlab.com/runner/fleet_scaling/ ; https://docs.gitlab.com/api/runners/ ; https://docs.gitlab.com/ci/runners/configure_runners/ |
| Buildkite Agent | CI runner | 원라이너 `TOKEN=... bash -c "$(curl -sL .../install.sh)"`로 설치합니다. 설치 스크립트는 SHA256을 검증하고 서비스는 설치하지 않습니다. agent 토큰은 클러스터 범위의 장기 토큰이며, 선택적으로 `expires_at`을 둘 수 있습니다(최소 10분 뒤, 변경 불가). 토큰 계층은 session 토큰에서 job 토큰으로 이어집니다. JAT(`bkjat_`)는 job 하나 전용이고 TTL은 1–3600초(기본값 900초)입니다. | IP allowlist를 지원합니다. 토큰을 폐기해도 에이전트는 끊기지 않지만, IP allowlist를 바꾸면 끊깁니다. | https://buildkite.com/docs/agent/v3/tokens ; https://buildkite.com/docs/agent/v3/linux ; https://raw.githubusercontent.com/buildkite/agent/main/install.sh ; https://buildkite.com/docs/agent/v3/ubuntu ; https://buildkite.com/docs/agent/self-hosted/job-acquisition-tokens ; https://buildkite.com/docs/pipelines/security/clusters/manage |
| kubeadm bootstrap tokens | 클러스터 조인 | 토큰 형식은 `[a-z0-9]{6}.[a-z0-9]{16}`(공개 ID와 시크릿)입니다. TTL 기본값은 24시간이며, v1.8 이전에는 기본값이 0(무기한)이었습니다. 조인 명령은 `kubeadm token create --print-join-command`로 만들고, `--discovery-token-ca-cert-hash sha256:`로 CA를 고정합니다. | kubelet CSR은 `kubectl certificate approve`로 승인합니다. 수동 승인 모드에서 관리자에게는 `system:bootstrap:<id>`만 보입니다. 토큰을 공유하면 JWS 위조와 MITM이 가능해집니다. | https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/reference/access-authn-authz/bootstrap-tokens.md ; https://github.com/kubernetes/kubernetes/blob/d285472482503c249133440979b51e49949030ea/cmd/kubeadm/app/apis/bootstraptoken/v1/defaults.go ; https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/reference/setup-tools/kubeadm/kubeadm-join.md ; https://raw.githubusercontent.com/kubernetes/design-proposals-archive/main/cluster-lifecycle/bootstrap-discovery.md ; https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/reference/access-authn-authz/kubelet-tls-bootstrapping.md |
| k3s | 클러스터 조인 | `curl -sfL https://get.k3s.io \| K3S_URL=... K3S_TOKEN=... sh -`로 설치하며, env 파일(0600)을 씁니다. 전체 토큰 `K10<cahash>::creds`는 CA를 고정하지만, 짧은 토큰은 CA 검증을 건너뜁니다. `k3s token create --ttl`의 기본값은 24시간입니다. | server 토큰은 관리자 권한을 주고 datastore 키로도 쓰입니다. agent 토큰은 만료되지 않습니다. | https://raw.githubusercontent.com/k3s-io/docs/main/docs/cli/token.md ; https://github.com/k3s-io/k3s/blob/65d71277db7a81c2d498bfbb62e198102889a795/pkg/clientaccess/token.go ; https://github.com/k3s-io/k3s/blob/65d71277db7a81c2d498bfbb62e198102889a795/install.sh |
| Rancher cluster import | 클러스터 조인 | `kubectl apply -f /v3/import/<token>_<id>.yaml`로 가져오며, `curl --insecure`를 쓰는 변형도 있습니다. | GHSA-7r53: 토큰이 평문으로 저장되고 만료되지 않았으며, 설치 스크립트 권한이 0644였습니다. 2.15 수정 이후 토큰은 첫 사용 30초 뒤 또는 발급 30분 뒤에 만료됩니다. GHSA-23h9는 토큰 존재 여부를 열거할 수 있는 oracle 취약점입니다. | https://github.com/rancher/rancher/blob/7b1f21008e6f00afc802492b4d5dcd2544525aa3/pkg/controllers/dashboard/clusterregistrationtoken/status.go ; https://github.com/rancher/rancher/security/advisories/GHSA-7r53-jvhg-9jq4 ; https://github.com/rancher/rancher/security/advisories/GHSA-23h9-rr79-r3gh ; https://raw.githubusercontent.com/rancher/rancher-docs/main/docs/how-to-guides/new-user-guides/kubernetes-clusters-in-rancher-setup/register-existing-clusters.md |
| Chrome Remote Desktop (headless) | 원격 데스크톱 | `remotedesktop.google.com/headless`에서 `start-host --code="4/..." --redirect-url --name` 명령을 만들어 줍니다. code는 일회용 OAuth 코드이고 유효시간은 "몇 분"이라고만 되어 있습니다(정확한 TTL은 공개되지 않음). PIN은 6자리 이상이며 HMAC으로 저장됩니다. 2단계 교환에서 사용자 refresh token은 폐기되고 서비스 계정 토큰으로 바뀝니다. 다시 실행하면 기존 호스트를 대체합니다. 설치 가이드는 `sudo -k`를 씁니다. | 소유자에게 승인 프롬프트나 기기 정보가 표시되지 않습니다. 지원 모드는 일회용 코드를 쓰고, 도와주는 사람의 이메일을 보여 주며, 30분마다 다시 확인합니다. RFC 6749는 인가 코드를 최대 10분, 일회용으로 쓰도록 권고합니다. | https://cloud.google.com/architecture/chrome-desktop-remote-on-compute-engine ; https://docs.cloud.google.com/architecture/chrome-desktop-remote-windows-compute-engine ; https://chromium.googlesource.com/chromium/src/+/main/remoting/host/setup/start_host_main.cc ; https://chromium.googlesource.com/chromium/src/+/main/remoting/host/setup/oauth_host_starter.cc ; https://chromium.googlesource.com/chromium/src/+/main/remoting/host/setup/host_starter_base.cc ; https://chromium.googlesource.com/chromium/src/+/main/remoting/host/pin_hash.cc ; https://support.google.com/chrome/answer/1649523 ; https://support.google.com/chrome/a/answer/2799701 ; https://bytexd.com/install-chrome-remote-desktop-headless/ ; https://www.rfc-editor.org/rfc/rfc6749.txt |
| RustDesk | 원격 데스크톱 | `--assign --token`은 `/api/devices/cli`로 POST합니다. `--deploy`는 `{id,uuid,pk}`를 보내고 종료 코드 3/5/6을 씁니다. 서버에 "Require deployment" 설정이 있습니다. 토큰 TTL은 명시되어 있지 않습니다. | 기기 ID는 인증 수단이 아닙니다. 배포 스크립트가 비밀번호를 출력합니다. | https://github.com/rustdesk/doc.rustdesk.com/blob/56f9310ca2c91f8e94b6a460290d1e94f62e0482/content/self-host/client-deployment/_index.en.md ; https://github.com/rustdesk/rustdesk/blob/828d1101afd2e656eb1c1d18511009bdf5336238/src/core_main.rs ; https://github.com/rustdesk/doc.rustdesk.com/blob/56f9310ca2c91f8e94b6a460290d1e94f62e0482/content/self-host/rustdesk-server-pro/console/_index.en.md ; https://github.com/rustdesk/doc.rustdesk.com/blob/56f9310ca2c91f8e94b6a460290d1e94f62e0482/content/self-host/client-configuration/advanced-settings/_index.en.md |
| TeamViewer | 원격 데스크톱 | `TeamViewer.exe assignment --id <AssignmentId> --device-alias`로 할당하며, 오류 코드가 정해져 있습니다(예: 407, 408). 예전 방식은 `--api-token --grant-easy-access`였습니다. | 커뮤니티 스크립트는 `sleep 30`에 의존합니다. | https://github.com/teamviewer/TeamViewerPS/blob/da52a7d1803bfea04ca35f643fc2828f507a0f92/Cmdlets/Public/Add-TeamViewerAssignment.ps1 ; https://github.com/teamviewer/TeamViewerPS/blob/da52a7d1803bfea04ca35f643fc2828f507a0f92/Cmdlets/Private/Resolve-TeamViewerAssignmentErrorCode.ps1 ; https://github.com/Neon6105/intune-scripts-linux/blob/53c87f5820ec34054261aa05b32ede613a2a4f18/install-teamviewer-deb.sh ; https://github.com/amidaware/community-scripts/blob/763e7ed95c799466baa43fa17487eabb3a7ab66a/scripts/Win_Teamviewer_Host_custom_install.ps1 |
| AnyDesk | 원격 데스크톱 | `echo pw \| anydesk --set-password`로 비밀번호를 stdin으로 넘기고, `--get-id`로 ID를 조회합니다. | 저장된 토큰은 기기에 남아 있다가 비밀번호를 바꾸면 지워집니다. 2024년에 프로덕션 서버 침해가 있었고 비밀번호를 초기화했습니다. | https://support.anydesk.com/docs/command-line-interface-for-windows ; https://support.anydesk.com/docs/command-line-interface-for-linux ; https://support.anydesk.com/docs/unattended-access ; https://www.bleepingcomputer.com/news/security/anydesk-says-hackers-breached-its-production-servers-reset-passwords/ |
| Syncthing | P2P 동기화 | 기기마다 Ed25519 키가 있습니다. 기기 ID는 인증서의 SHA-256을 base32로 표기하고 Luhn 검사 문자를 붙인 56자 값이며, 공개 정보입니다. 대기 중인 기기는 `GET`/`DELETE /rest/cluster/pending/devices`와 `PendingDevicesChanged` 이벤트로 다룹니다. | 양쪽 모두 서로를 추가해야 합니다. GUI는 Add / Ignore / Dismiss를 제공하며, 표시되는 이름은 상대가 스스로 정한 것입니다. introducer 신뢰는 전이됩니다. `autoAcceptFolders`는 신뢰한 기기에만 적용되고, `untrusted`로 표시하면 introducer와 자동 수락 권한이 제거됩니다. 짧은 ID와 PIN을 쓰는 방식이 제안됐지만 구현되지 않았습니다. | https://docs.syncthing.net/dev/device-ids.html ; https://raw.githubusercontent.com/syncthing/syncthing/main/lib/protocol/deviceid.go ; https://docs.syncthing.net/users/faq.html ; https://raw.githubusercontent.com/syncthing/syncthing/main/lib/model/model.go ; https://docs.syncthing.net/rest/cluster-pending-devices-get.html ; https://raw.githubusercontent.com/syncthing/syncthing/main/gui/default/index.html ; https://docs.syncthing.net/users/introducer.html ; https://docs.syncthing.net/users/config.html ; https://docs.syncthing.net/users/security.html |
| Plex Media Server | 셀프호스팅 미디어 claim | `PLEX_CLAIM` 또는 `PLEX_CLAIM_FILE`을 `POST api.plex.tv/api/claim/exchange`로 교환하면 `Preferences.xml`에 `PlexOnlineToken`이 저장됩니다. TTL은 linuxserver 문서 기준 "4분", 공식 문서 기준 "몇 분"입니다. | 일회용 제한은 클라이언트 쪽에서만 걸려 있습니다. 교환에 실패해도 조용히 건너뜁니다. | https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/README.md ; https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/root/etc/cont-init.d/40-plex-first-run ; https://github.com/linuxserver/docker-plex/blob/master/README.md ; https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/charts/plex-media-server/values.yaml ; https://github.com/plexinc/pms-docker/pull/142 ; https://github.com/pkkid/python-plexapi/blob/master/plexapi/myplex.py ; https://github.com/hotio/plex/blob/release/root/etc/s6-overlay/s6-rc.d/init-setup-app/run |
| Home Assistant onboarding | 첫 실행 claim | `POST /api/onboarding/users`는 인증이 없고 lock으로만 보호됩니다(PR #169299). | 먼저 접속한 사람이 소유자가 됩니다. | https://github.com/home-assistant/core/blob/e40dde763b8243aa6a26db0e28926fb8dc633e22/homeassistant/components/onboarding/views.py ; https://github.com/home-assistant/core/pull/169299 |
| Jellyfin startup wizard | 첫 실행 claim | 마법사가 끝나기 전까지 `/Startup/*`에는 인증이 없습니다. | PR #17369가 마법사가 다시 열리는 버그를 고쳤습니다. Issue #17880은 provision 파일을 제안합니다. | https://github.com/jellyfin/jellyfin/blob/c7ce6a1b56ee7dcc1896250df0f2f9deaa828a81/Jellyfin.Api/Auth/FirstTimeSetupPolicy/FirstTimeSetupHandler.cs ; https://github.com/jellyfin/jellyfin/pull/17369 ; https://github.com/jellyfin/jellyfin/issues/17880 |
| RFC 8628 (Device Authorization Grant) | 표준 | 필드는 `device_code`, `user_code`, `expires_in`, `interval`(기본값 5초)입니다. `slow_down`을 받으면 간격을 5초 늘립니다. base-20 알파벳 8자 user code는 약 34.5비트이며, 추측 시도를 약 5회까지 허용하는 수준입니다. | §5.4(원격 피싱)는 승인자에게 기기 정보를 보여 주라고 권고합니다. | https://www.rfc-editor.org/rfc/rfc8628.txt |
| IETF cross-device security BCP draft | 표준(초안) | 근접성 확인, 거절을 기본값으로 두기, device flow는 최후의 수단으로만 쓰기를 권고합니다. | 공격자가 필요할 때마다 코드를 새로 만들면 짧은 TTL은 방어 효과가 없습니다. | https://github.com/oauth-wg/oauth-cross-device-security/blob/de72489b8b3a2520182e2cb80fedf2010c433ab8/draft-ietf-oauth-cross-device-security.md#L1058-L1213 ; https://www.ietf.org/archive/id/draft-ietf-oauth-cross-device-security-16.txt |
| GitHub device flow / gh CLI | Device flow | device code는 900초, user code는 8자입니다. 앱마다 시간당 50회까지 시도할 수 있습니다. access token은 8시간, refresh token은 6개월짜리입니다. `cli/oauth`는 poll 간격을 1.2배씩 늘립니다. gh는 자격증명을 credential store에 저장합니다. | 사용자가 github.com에서 코드를 입력합니다. | https://raw.githubusercontent.com/github/docs/main/content/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps.md ; https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps ; https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app ; https://github.com/cli/oauth/blob/7efb8e657cd3a69387b05401be9ce36fa464d3ba/device/device_flow.go ; https://github.com/cli/cli/blob/ed84a97a559956853e4464fee3c0ff4262a0d6ac/pkg/cmd/auth/login/login.go |
| Microsoft Entra / Azure CLI | Device flow | device code는 15분짜리이고 `verification_uri_complete`를 제공하지 않습니다. | Microsoft는 Conditional Access로 device code flow를 차단하라고 권고합니다. 실제 피싱에 악용되었습니다(Storm-2372, Volexity). | https://raw.githubusercontent.com/MicrosoftDocs/entra-docs/main/docs/identity-platform/v2-oauth2-device-code.md ; https://raw.githubusercontent.com/MicrosoftDocs/entra-docs/main/docs/identity/conditional-access/concept-authentication-flows.md ; https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code ; https://www.microsoft.com/en-us/security/blog/2025/02/13/storm-2372-conducts-device-code-phishing-campaign/ ; https://www.volexity.com/blog/2025/02/13/multiple-russian-threat-actors-targeting-microsoft-device-code-authentication/ ; https://raw.githubusercontent.com/MicrosoftDocs/azure-docs-cli/main/docs-ref-conceptual/Latest-version/includes/interactive-login.md |
| Keycloak | Device flow(IdP) | device code 수명은 600초, interval은 5초입니다. | 코드를 일회용 저장소에 보관합니다. | https://github.com/keycloak/keycloak/blob/f7532e18737e6288bc139acf2bc7373e81af6404/server-spi/src/main/java/org/keycloak/models/OAuth2DeviceConfig.java ; https://github.com/keycloak/keycloak/blob/f7532e18737e6288bc139acf2bc7373e81af6404/services/src/main/java/org/keycloak/protocol/oidc/grants/device/endpoints/DeviceEndpoint.java |
| Ory Hydra | Device flow(IdP) | device code는 10분짜리이고, user code는 A–Z 8자입니다. | `ShouldRateLimit`의 기본값은 false입니다. | https://github.com/ory/hydra/blob/4174065ffb052799890f7480f5360a877a67ffc1/fosite/config_default.go |
| Google OAuth for limited-input devices | Device flow | device code는 1800초짜리이고, 비표준 필드명 `verification_url`을 씁니다. | 표준 device flow 동의 절차를 따릅니다. | https://developers.google.com/identity/protocols/oauth2/limited-input-device.md.txt |
| gcloud | CLI 로그인 | `gcloud auth login --no-browser` / `--remote-bootstrap`을 지원합니다. | 확인 프롬프트의 기본값이 No입니다. | https://github.com/google-cloud-sdk-unofficial/google-cloud-sdk/blob/1f04d1ec3a8e0c0b9681fd1efc2ef71aa2d1502c/lib/googlecloudsdk/command_lib/auth/flags.py ; https://github.com/google-cloud-sdk-unofficial/google-cloud-sdk/blob/1f04d1ec3a8e0c0b9681fd1efc2ef71aa2d1502c/lib/googlecloudsdk/core/credentials/flow.py |
| flyctl | CLI 로그인 | PKCE loopback을 쓰고, 대체 경로로 `--code`를 붙여넣는 방식이 있습니다. 세션은 15분입니다. | headless 환경에서는 PKCE 이전의 구형 flow를 거부합니다. | https://github.com/superfly/flyctl/blob/9cca2c596271cdc163784c0dbcc68b420ade6e75/internal/command/auth/webauth/webauth.go ; https://docs.fly.io/flyctl/cmd/fly_auth_login.md |
| OpenAI Codex CLI | CLI 로그인 | `codex login --device-auth`는 PKCE verifier로 바인딩됩니다. | "누군가 이 코드를 줬다면 취소하라"는 경고를 표시합니다. | https://developers.openai.com/codex/auth.md ; https://github.com/openai/codex/blob/f3a90545aed3188ee1e5c119ec89a742de399cdc/codex-rs/login/src/device_code_auth.rs |
| Claude Code | CLI 로그인 | 대체 경로로 코드를 붙여넣는 방식이 있습니다. 자격증명은 Keychain 또는 `~/.claude/.credentials.json`(0600)에 저장됩니다. `setup-token`은 1년짜리 토큰을 만듭니다. | headless 용도의 장기 토큰입니다. | https://code.claude.com/docs/en/authentication.md |
| Happy coder | 폰-터미널 페어링 | QR 코드 `happy://terminal?<X25519 pubkey>`를 쓰고, CLI는 1초마다 poll합니다. `~/.happy/access.key`는 권한을 명시하지 않고 기록합니다. | 서버에서는 먼저 승인한 사람이 이기고 TTL이 없습니다. 폰은 기기 정보 없이 맹목적으로 승인합니다. | https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/ui/auth.ts ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-server/sources/app/api/routes/authRoutes.ts ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-app/sources/hooks/useConnectTerminal.ts ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/persistence.ts |
| Signal linked devices / signal-cli | QR 기기 연결 | `sgnl://linkdevice?uuid&pub_key` 형식을 쓰고, `signal-cli link`로 연결합니다. QR은 45초마다 최대 6회 바뀐다고 알려져 있습니다(회전 동작은 미검증). | 주 기기인 폰이 QR을 스캔해 승인합니다. | https://raw.githubusercontent.com/signalapp/Signal-Desktop/main/ts/textsecure/Provisioner.preload.ts ; https://raw.githubusercontent.com/AsamK/signal-cli/master/man/signal-cli.1.adoc |
| Balena | IoT fleet | fleet provisioning key를 이미지에 넣어 재사용합니다. `expiryDate`는 null일 수 있고 `--provisioning-key-expiry-date`로 지정합니다. 자격증명은 `/mnt/boot/config.json`에 있습니다. | 프로비저닝이 끝나면 키를 삭제하고 범위가 좁은 device key로 바꿉니다. | https://raw.githubusercontent.com/balena-io/docs/a0d2eb7d44495b1bc835db38b2417f52332e71ff/pages/learn/welcome/security.md ; https://raw.githubusercontent.com/balena-io/balena-cli/e55eda049d6ae23a603700d7f6eee16783c350e3/src/commands/config/generate.ts ; https://raw.githubusercontent.com/balena-io/balena-sdk/master/src/models/application.ts ; https://raw.githubusercontent.com/balena-io/balena-cli/e55eda049d6ae23a603700d7f6eee16783c350e3/src/utils/config.ts |
| AWS IoT fleet provisioning | IoT fleet | claim 인증서의 정책은 프로비저닝 토픽만 허용합니다. ownership token은 1시간짜리입니다. CSR을 쓰면 개인키가 기기에만 남습니다. trusted-user claim 인증서는 5분짜리입니다. | pre-provisioning Lambda hook으로 승인하거나 거부할 수 있습니다. claim 인증서를 폐기해도 이미 프로비저닝된 기기에는 영향이 없습니다. | https://docs.aws.amazon.com/iot/latest/developerguide/provision-wo-cert.html ; https://docs.aws.amazon.com/iot/latest/developerguide/fleet-provision-api.md ; https://docs.aws.amazon.com/iot/latest/apireference/API_CreateProvisioningClaim.md ; https://docs.aws.amazon.com/iot/latest/developerguide/iot-provision.md |
| Particle | IoT claim | claim 전용 OAuth 클라이언트가 `/v1/device_claims`에서 claim code를 발급합니다. 기기 추가는 `particle device add`로 합니다. | 기기를 이전하려면 현재 소유자의 이메일 승인이 필요합니다. 격리(quarantine) 대기열이 있습니다. | https://raw.githubusercontent.com/particle-iot/docs/5bb967fe0f95b17dfaa55ffc1628c11d095c2a67/src/content/getting-started/cloud/device-claiming.md ; https://raw.githubusercontent.com/particle-iot/particle-api-js/7bf081abf8ccbb05904895ac1b32d1b45ac6af61/src/Particle.ts |
| Mender | IoT OTA | `mender-setup --tenant-token`으로 설정합니다. tenant token은 재사용할 수 있지만 대기(pending) 상태의 기기만 만듭니다. device JWT는 1주일짜리입니다. | 관리자가 수락하기 전까지 auth set(식별 정보와 공개키)은 pending 상태입니다. 사전 승인은 공개키가 정확히 일치해야 합니다. | https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/03.Client-installation/02.Install-with-Debian-package/docs.md ; https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/02.Overview/14.Device-authentication/docs.md ; https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/10.Server-integration/02.Preauthorizing-devices/docs.md ; https://raw.githubusercontent.com/mendersoftware/mender-server/2a52456e94112896bcbf1310a0cabd9440434647/backend/services/deviceauth/devauth/devauth.go |
| argv / shell-history 노출(참고 자료) | 공통 | `/proc/<pid>/cmdline`을 통해 다른 프로세스가 argv를 볼 수 있습니다. 시크릿을 넣어 입력한 명령은 Bash history에 남습니다. | 시크릿을 argv 대신 파일, stdin, env로 넘겨야 하는 근거입니다. | https://man7.org/linux/man-pages/man5/proc_pid_cmdline.5.html ; https://www.gnu.org/software/bash/manual/html_node/Bash-Variables.html |

### 반복 패턴

- **짧은 수명의 키를 머신이 생성한 장기 신원으로 교환**
  - 부트스트랩 시크릿은 공개키를 한 번 등록하는 데만 쓰고 저장하지 않습니다.
  - 사례: GitHub Actions(RSA-2048, 공개키만 업로드), Netdata(RSA-2048), Balena(provisioning key 삭제 후 device key로 교체), AWS IoT(CSR, 개인키는 기기에 남음), Chrome Remote Desktop(사용자 refresh token 폐기, 서비스 계정 토큰 보관), Plex(claim을 `PlexOnlineToken`으로 교환), Tailscale(auth key 미저장), Teleport `bound_keypair`, Mender(auth set).
- **일회용·짧은 TTL의 조인 키**
  - Headscale: 일회용, 기본 1시간
  - NetBird Add Peer: 일회용, 24시간
  - kubeadm: 24시간
  - Teleport: 30분, 단 TTL 안에서는 재사용 가능
  - GitHub Actions: 1시간
  - Buildkite JAT: 900초
  - Rancher(수정 후): 첫 사용 30초 뒤 또는 발급 30분 뒤
  - Chrome Remote Desktop, Plex: "몇 분"
- **재사용 가능하고 만료되지 않는 조직/fleet 토큰**
  - 사례: cloudflared, ngrok, Datadog, Netdata Space 토큰, Portainer `EDGE_KEY`, k3s agent 토큰, Balena(`expiryDate` null), Mender tenant token, Buildkite agent 토큰, Grafana(만료일 미지정 시).
  - 이런 토큰이 안전하려면 pending 승인(Mender, Portainer)이나 좁은 권한 범위와 함께 써야 합니다.
- **pending 대기열과 관리자 승인**
  - 사례: Tailscale device approval(웹훅 포함), NetBird peer approval, ZeroTier "Auth?", Portainer waiting room, Syncthing pending devices(REST와 이벤트), Mender auth set, kubelet CSR 승인, Particle quarantine, Headscale `auth register`.
- **키를 폐기해도 이미 등록된 머신은 제거되지 않음**
  - 사례: Tailscale, NetBird, Buildkite, cloudflared, AWS IoT.
  - 머신 제거는 별도 작업으로 해야 합니다. cloudflared는 연결을 `DELETE`해야 하고, Buildkite는 IP allowlist를 바꿔야 에이전트가 끊깁니다.
- **저장 시 해시, 표시는 한 번만**
  - 해시 저장: NetBird(`sha256`, 5자 마스크), Headscale(접두사와 해시), Chrome Remote Desktop PIN(HMAC).
  - 한 번만 표시: ngrok, GitLab.
  - 반례: Rancher가 토큰을 평문으로 저장(GHSA-7r53).
- **`id.secret` 분리형 토큰 또는 토큰 안의 CA pin**
  - TOFU 대신 클라이언트가 서버를 검증하게 합니다.
  - 사례: kubeadm `[a-z0-9]{6}.[a-z0-9]{16}`와 `--discovery-token-ca-cert-hash`, k3s `K10<cahash>::`, Teleport 설치 스크립트에 넣은 CA pin, Portainer `EDGE_KEY`의 fingerprint.
- **시크릿을 argv 대신 파일, stdin, env로 전달**
  - 사례: cloudflared `--token-file`(argv 노출 수정), Tailscale `--auth-key=file:`, Plex `PLEX_CLAIM_FILE`, AnyDesk `--set-password`(stdin), Teleport `--token=<file>`, k3s env 파일(0600), Grafana systemd `env.conf`(600).
  - 근거: `/proc/<pid>/cmdline`과 bash history.
- **RFC 8628 device flow와 피싱 방어**
  - 구현 사례: GitHub(900초, 8자), Entra(15분), Keycloak(600초), Hydra(10분), Google(1800초), gh, Codex(PKCE와 "누가 줬다면 취소" 경고), flyctl(PKCE, 구형 flow 거부), gcloud(확인 기본값 No).
  - RFC 8628 §5.4와 IETF BCP의 권고: 기기 정보를 보여 주고, 거절을 기본값으로 두고, 근접성을 확인합니다.
  - Microsoft는 Storm-2372 피싱 이후 이 flow를 아예 차단하라고 권고합니다.
- **설치 스크립트 강화**
  - 체크섬 검증: Buildkite(SHA256), Grafana(SHA-256).
  - `main()` 래퍼: Tailscale.
  - `--proto =https`: NetBird.
  - 서비스 설치: GitHub는 macOS에서 sudo 없이 사용자별 LaunchAgent로 설치합니다.
  - 정해진 종료 코드: Netdata(12/13), RustDesk(3/5/6), TeamViewer(407/408).
- **인증 없는 "먼저 온 사람이 소유" claim**
  - 사례: Home Assistant, Jellyfin, Happy 서버, Portainer TOFU.
  - Jellyfin은 마법사가 다시 열리는 버그가 있었고, issue #17880에서 provision 파일이 해결책으로 제안되었습니다.
- **회전·만료 관리**
  - 비활성 정리: GitHub Actions는 오프라인 runner를 14일 뒤 삭제하고, GitLab은 7일 뒤 stale로 표시합니다.
  - 키 만료: Tailscale node key는 180일 뒤 만료됩니다.
  - 폐기: GitLab registration token은 17.0부터 410을 반환합니다.

### 상충되는 주장

- **Teleport 기본 토큰 TTL**: 소스 코드는 `tctl` 30분, Web UI 4시간입니다(https://raw.githubusercontent.com/gravitational/teleport/master/lib/defaults/defaults.go). 반면 issue에는 예전 도움말 기준 기본 1시간, 최대 48시간으로 적혀 있습니다(https://github.com/gravitational/teleport/issues/11137). **소스 코드를 따라 30분(tctl), 4시간(Web UI)으로 봅니다.** issue는 예전 도움말을 반영한 것입니다.
- **NetBird setup key 기본값**: 문서는 Add Peer가 일회용 24시간 키를 만들고 대화상자 기본값이 7일이라고 합니다(https://docs.netbird.io/how-to/register-machines-using-setup-keys). 코드상 서버 기본 키는 재사용 가능한 30일짜리입니다(https://github.com/netbirdio/netbird/blob/main/management/server/types/setupkey.go). 서로 다른 화면의 값이라 실제 충돌은 아니며, 30일 서버 기본값은 코드로 확인됩니다.
- **Plex claim TTL**: linuxserver 문서는 "4분"(https://github.com/linuxserver/docker-plex/blob/master/README.md), 공식 문서는 "몇 분"(https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/README.md)이라고 합니다. 부분적으로만 확인되었으므로 약 4분, 몇 분 수준으로 봅니다.
- **Plex claim 호스트**: hotio 이미지는 `plex.tv`로 POST하고(https://github.com/hotio/plex/blob/release/root/etc/s6-overlay/s6-rc.d/init-setup-app/run), 공식 이미지는 `api.plex.tv`로 POST합니다(https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/root/etc/cont-init.d/40-plex-first-run). 공식 이미지의 `api.plex.tv`를 기준으로 봅니다.
- **Netdata 토큰 유효성**: 2020년 글은 "이전 토큰도 계속 유효하다"고 합니다(https://community.netdata.cloud/t/automate-netdata-cloud-node-claim-on-instance-deploy/25). 이후 글은 재발급하면 이전 토큰이 무효화된다고 합니다(https://community.netdata.cloud/t/is-the-token-for-claiming-new-nodes-different-each-time/1026). 현재 동작은 재발급 시 무효화이며, 2020년 글은 낡은 정보입니다.
- **ngrok 무료 플랜의 서비스 실행**: 2022년 issue는 허용되지 않는다고 합니다(https://github.com/inconshreveable/ngrok/issues/839). 현재 무료 플랜 제한 문서에는 그런 제한이 없습니다(https://ngrok.com/docs/pricing-limits/free-plan-limits.md). issue가 낡은 정보일 가능성이 높습니다.
- **Chrome Remote Desktop PIN 길이**: 가이드는 "6자리"라고 하고(https://cloud.google.com/architecture/chrome-desktop-remote-on-compute-engine), 소스 코드는 "최소 6자리"입니다(https://chromium.googlesource.com/chromium/src/+/main/remoting/host/pin_hash.cc). 소스 코드를 따라 최소 6자리로 봅니다.
- **Cloudflare 토큰 작업 명칭**: 한 문서는 "Refresh token"(https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel/index.md), 다른 문서는 "Rotate token"(https://developers.cloudflare.com/tunnel/reference/tunnel-tokens/index.md)이라고 부릅니다. 이름만 다를 뿐 같은 작업이며, 기존 connector는 유지됩니다.

## 2.2 원격 설치·경쟁 앱

| 제품 | 분류 | 방식(명령/TTL/기본값) | 승인·보안 | 출처 |
|---|---|---|---|---|
| VS Code Remote-SSH | SSH 자동 설치형 IDE 서버 | 원격에서 먼저 다운로드하고, 실패하면 로컬에서 받아 전송합니다(`remote.SSH.localServerDownload`). artifact는 commit에 고정됩니다(`commit:<sha>/server-<plat>/stable`). COMSPEC을 본 뒤 `uname -s`/`uname -m`로 플랫폼을 판별합니다. 설치 스크립트는 base64로 `bash -l`에 넘기며 `flock -x -w 30`으로 보호합니다. 경로는 `~/.vscode-server/code-<commit>`과 `cli/servers/Stable-<commit>`입니다. 다운로드 캐시는 5개(LRU)이고 staging 후 atomic rename합니다. keepalive 5s, timeout 20s, reconnect grace 3h이며 SSH 연결을 2개 씁니다. | connection token은 0600 파일에 든 UUID이고 `--connection-token-file`로 전달합니다. `x-sha256` 헤더는 오지만 검증하지 않습니다. enrollment와 repo discovery는 없습니다. | https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/faq.md ; https://github.com/jeanp413/open-remote-ssh/blob/1911064579e9b6162ee90bcd20bf8d1b9d5af552/src/scripts/server-setup.sh ; https://github.com/microsoft/vscode/blob/9a68ad723966f70414e9bdb325ec1156f51a32b2/cli/src/download_cache.rs ; https://github.com/microsoft/vscode-remote-release/issues/11136 ; https://code.visualstudio.com/docs/remote/troubleshooting |
| VS Code Tunnels (`code tunnel`) | 아웃바운드 터널 + device-code 로그인 | GitHub device code를 씁니다. client는 `01ab8ac9400c4e429b23`, scope는 `read:user+read:org`입니다. 5s마다 polling하고 `slow_down`이면 +5s, `expires_in=900`입니다. 터널 이름은 hostname에서 따온 `[\w-]+` 최대 20자이고 fallback은 `remote-machine`입니다. 10개 한도를 넘으면 유휴 터널 하나를 *무작위로* 삭제합니다. `service install`은 Linux에서 systemd user unit(`Restart=always`, `RestartSec=10`)을 만들고 SSH 접속 중일 때만 linger 힌트를 보여 줍니다. macOS는 `LimitLoadToSessionType=Aqua` plist, Windows는 HKCU Run을 씁니다. 유휴 dev tunnel은 30일 뒤 삭제됩니다. | token은 keyring(`vscode-cli` prefix)에 저장하고, 안 되면 0600 `token.json`에 둡니다. 비대화형으로는 `--access-token`/`--refresh-token`을 씁니다. 연결은 아웃바운드 전용이고 E2E AES-256-CTR로 암호화됩니다. | https://code.visualstudio.com/docs/remote/tunnels ; https://github.com/microsoft/vscode/blob/main/cli/src/auth.rs ; https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/dev_tunnels.rs ; https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/service_linux.rs ; https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/faq |
| Zed remote | SSH 자동 설치 서버 | 시스템 `ssh`를 ControlMaster와 `SSH_ASKPASS_REQUIRE=force`로 쓰고, `ssh -O check`로 기존 master를 재사용합니다. 바이너리는 `~/.zed_server/zed-remote-server-<channel>-<version>`이며 버전이 정확히 일치해야 합니다. 원격에서 curl(안 되면 wget)로 받고, 그래도 안 되면 SFTP/scp로 업로드합니다(`upload_binary_over_ssh`). 이후 gunzip, chmod, atomic move 순서입니다. 10분 유휴 시 종료되고 heartbeat 5s, 재연결 3회입니다. 호스트 목록은 `read_ssh_config`에서 git 호스트를 걸러 만듭니다. deep link는 `zed ssh://`와 `zed://ssh/`입니다. | checksum이 없고 세션 범위로만 동작합니다. 호스트 전체 repo 스캔은 하지 않습니다. | https://zed.dev/docs/remote-development.md ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/remote/src/transport/ssh.rs#L1146-L1330 ; https://zed.dev/docs/git.md ; https://github.com/zed-industries/zed/issues/34621 |
| JetBrains Gateway | SSH 백엔드 배포 | `~/.cache/JetBrains/RemoteDev/dist`에 설치하며 원격 다운로드, custom link, 업로드 중 하나를 씁니다. preflight로 `AllowTcpForwarding`과 디스크(다운로드 크기의 4배 이상)를 확인하고, 상태를 60s 기다립니다. `registerBackendLocationForGateway`를 지원합니다. recents에 Running 배지가 붙고, project path는 자유 입력입니다. | join link는 `jt`와 `fp` fingerprint를 담고, `fp`가 없으면 거부합니다. offline 모드에서는 PGP KEYS로 검증합니다. | https://www.jetbrains.com/help/idea/remote-development-a.html ; https://www.jetbrains.com/help/idea/remote-development-troubleshooting.html ; https://youtrack.jetbrains.com/api/issues/IJPL-187560?fields=summary,description ; https://www.jetbrains.com/help/idea/fully-offline-mode.html |
| JetBrains Toolbox Agent | Gateway 후속 | Gateway는 deprecated입니다. SSH auto-deploy는 "will be eventually restricted"로 예고됐고, URL scheme은 `jetbrains://gateway/` 하나만 씁니다. | 호스트 정책은 `environment.json`에 둡니다(`allowInstallation`, `tools.location` 레벨, `allowPortForwarding`). | https://www.jetbrains.com/help/toolbox-app/jetbrains-gateway-migrations-guide.html |
| Cursor Remote-SSH | VS Code fork, SSH | `~/.cursor-server/cli/servers/Stable-<commit>/`에 설치합니다. `cursor tunnel`도 있습니다. | 원격 IP를 GeoIP로 차단합니다. 알려진 실패는 stale process로 인한 timeout과 lock 실패입니다. glibc가 2.28 미만이면 일반적인 "aborted" 오류만 나옵니다. | https://forum.cursor.com/t/cursor-ssh-keeps-downloading-vscode-server/158596 ; https://forum.cursor.com/t/help-cursor-remote-ssh-fails-with-error-resolving-ssh-authority-this-operation-was-aborted/154281 ; https://docs.rc.fas.harvard.edu/kb/cursor-remote-development/ |
| Windsurf / Devin Desktop SSH | VS Code fork, SSH | 자체 SSH 구현을 쓰며 원격은 Linux만 지원합니다. | agent forwarding이 기본으로 켜져 있습니다. | https://docs.windsurf.com/windsurf/advanced |
| open-remote-ssh | OSS Remote-SSH | 다운로드 URL을 `serverDownloadUrlTemplate`으로 설정할 수 있습니다. | `enableAgentForwarding` 기본값이 true입니다. | https://raw.githubusercontent.com/jeanp413/open-remote-ssh/master/package.json |
| gh codespace ssh | 관리형 SSH | 키는 `~/.ssh/codespaces.auto`이고, 세션마다 공개키를 RPC로 push합니다. `--config`는 ssh_config stanza를 출력합니다. | 출력되는 stanza에 `StrictHostKeyChecking no`가 들어갑니다. 생성자만 접속할 수 있고, token은 생성할 때마다 바뀝니다. | https://cli.github.com/manual/gh_codespace_ssh ; https://github.com/cli/cli/blob/trunk/pkg/cmd/codespace/ssh.go ; https://docs.github.com/en/codespaces/reference/security-in-github-codespaces ; https://github.com/microsoft/vscode/issues/329938 |
| Coder | CDE, CLI 로그인 + agent | `coder login`이 `/cli-auth`를 열면 사용자가 token을 붙여 넣습니다. 외부 workspace는 `curl .../api/v2/init-script/linux/amd64 \| CODER_AGENT_TOKEN=<uuid> sh`로 붙입니다. agent는 temp dir에서 돌고 서비스로 등록하지 않습니다. 네트워크는 WireGuard/DERP입니다. `config-ssh`는 `Match !exec "coder connect exists"`가 든 START/END 관리 블록을 씁니다. repo는 `devcontainer.json`으로만 찾습니다. | `/cli-auth`에는 기기 표시도 승인/거부도 없습니다. 세션은 24h이고 갱신됩니다. API token은 약 100년짜리 `coder:all`까지 가능합니다. 자격 증명은 keyring `coder-v2-credentials`에 두며 Linux에서는 평문 파일입니다. agent token은 만료 없이 재사용되는 UUID입니다. init script에는 비밀이 없습니다. | https://raw.githubusercontent.com/coder/coder/main/cli/login.go ; https://raw.githubusercontent.com/coder/coder/main/site/src/pages/CliAuthPage/CliAuthPageView.tsx ; https://coder.com/docs/admin/users/sessions-tokens ; https://coder.com/docs/admin/templates/managing-templates/external-workspaces ; https://raw.githubusercontent.com/coder/coder/main/coderd/httpmw/workspaceagent.go ; https://raw.githubusercontent.com/coder/coder/main/cli/configssh.go |
| Coder agent API versioning | 버전 skew | major.minor 방식이며 현재 2.15입니다. 더 새로운 버전은 거부하고, `ping`은 "agent is outdated"로 degrade합니다. 릴리스는 매월 첫째 화요일입니다. | 버전 불일치를 조용히 넘기지 않고 명시적으로 호환성을 검사합니다. | https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/tailnet/proto/version.go ; https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/apiversion/apiversion.go ; https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/cli/ping.go ; https://coder.com/docs/reference/releases.md |
| DevPod | SSH provider + agent 주입 | `devpod provider add ssh --name -o HOST=`로 등록합니다. `init`은 `echo`, `uname`, `id -ru`, 쓰기 가능한 dir 또는 `sudo -nl`, `docker ps`를 preflight로 확인합니다. `inject.sh`는 ping/pong 후 agent를 다운로드하거나 stdin으로 stream하고, 버전 확인(ExistsCheck)을 거쳐 5분간 retry합니다. agent는 `/tmp/${USER}/devpod/agent`에 있고 20분 비활성 시 종료됩니다. workspace 목록은 로컬 JSON뿐입니다. | `-oStrictHostKeyChecking=no -oBatchMode=yes`로 실행하고, `.devpod` ssh_config alias에도 `StrictHostKeyChecking no`가 들어갑니다. | https://devpod.sh/docs/how-it-works/overview ; https://github.com/loft-sh/devpod-provider-ssh/blob/master/pkg/ssh/ssh.go ; https://github.com/loft-sh/devpod/blob/5a0efcbff6610ab114b421f68a890739a452e66b/pkg/inject/inject.sh ; https://github.com/loft-sh/devpod/blob/5a0efcbff6610ab114b421f68a890739a452e66b/pkg/workspace/list.go ; https://devpod.sh/docs/developing-in-workspaces/inactivity-timeout |
| Happy Coder | CLI의 QR/웹 페어링 | ephemeral X25519를 씁니다. QR `happy://terminal?<pubkey>`나 웹 `/terminal/connect#key=`(fragment에 넣고 `replaceState`로 지움)로 연결하며 CLI가 1s마다 polling합니다. machine ID는 클라이언트가 만든 UUID이고 충돌하면 409 `machine_id_taken`입니다. daemon은 서비스가 아니라 shell profile에서 띄웁니다. macOS에서는 keychain 때문에 launchd를 피합니다. repo는 세션 이력과 bash RPC로 돌린 `git worktree list`로 찾습니다(#1736 flood). | `/v1/auth/request`는 인증이 없습니다. 승인 뒤에는 poll할 때마다 새 token이 발급되고 TTL도 consumed 표시도 없습니다. 승인자에게는 key 앞 12자만 보입니다. token은 TTL 없이 영구 저장됩니다. 머신 전체 `bash` RPC가 있습니다. #1830 이슈는 `access.key` 권한 0644와 인증 없는 loopback을 지적합니다. | https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/ui/auth.ts ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-server/sources/app/api/routes/authRoutes.ts ; https://happy.engineering/docs/security/ ; https://github.com/slopus/happy/issues/1736 ; https://github.com/slopus/happy/issues/1830 |
| Orca (stablyai) | Worktree manager + SSH daemon | 첫 연결 때 자기 자신을 `~/.orca-remote/orcad-<ver>`에 설치합니다. 고정된 Node, UUID fence lock, SFTP를 쓰며 #9828은 orphan lock 이슈입니다. launchd/systemd 서비스는 없습니다. `scanNestedRepos`는 BFS이고 기본 depth 3(1~8), 최대 100 repo(1~500), timeout 500~30000 ms이며 `.git` marker와 `.gitignore`를 따릅니다. worktree는 `git worktree list --porcelain -z`로 읽고 외부 worktree는 숨깁니다. target generation과 tombstone을 둡니다. SSH 페어링은 터널을 거치는 loopback 전용입니다. | `ssh-host-keys.json`이라는 자체 TOFU 저장소를 쓰며, 불일치하면 자격 증명을 보내기 전에 실패합니다. `orca://pair?code=`는 24-byte bearer로, 첫 사용 전까지 재사용할 수 있고 사용하면 만료 없는 grant가 됩니다. relay `inviteToken`은 최대 10분이며 모바일 전용입니다. 폐기는 `credentialRevision`을 올려서 합니다. | https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/docs/site/content/docs/ssh.mdx ; https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/managed-server/install/orcad-remote-install.ts ; https://github.com/stablyai/orca/issues/9828 ; https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/ssh/ssh-host-key-store.ts ; https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/project-groups/nested-repo-discovery.ts ; https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/git/worktree-list-reader.ts |
| Superset | Worktree manager, SSH `hosts add` | `superset hosts add <target> [--no-service]`가 SSH 세션 하나에서 모두 처리합니다. `uname`, glibc/musl, `/etc/machine-id`(중복 판별)를 probe한 뒤 버전이 고정된 install script를 실행합니다. 이어 systemd user unit(`KillMode=process`) 또는 LaunchAgent를 설치하고 linger를 묻습니다. 그 뒤 2s 간격으로 최대 120s 동안 online이 될 때까지 polling합니다. desktop은 이 과정을 내장 터미널에서 실행합니다. 로그인은 loopback 51789 callback을 씁니다. daemon은 127.0.0.1에 바인딩하고 relay를 거칩니다. 매시간 rollback이 가능한 자동 업데이트를 합니다. untracked worktree import와 Device 필터가 있습니다. | 호스트 이름이 붙은 계정 API key를 stdin으로 넘기고(`--api-key=-`), 실패하면 revoke합니다. 계획 중인 방식은 `curl superset.sh/j/<tok> \| sh`입니다. 이 token은 10~15분짜리 1회용이며 GET으로는 소비되지 않고 POST로만 redeem됩니다. 승인 화면에는 hostname/OS/IP가 나옵니다. device code는 Storm-2372 피싱 때문에 fallback으로만 둡니다. | https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/packages/cli/src/commands/hosts/add/command.ts ; https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/plans/20261010-add-host-over-ssh.md ; https://github.com/superset-sh/superset/blob/eb9db3ed0e5d6586f67c8d51340bc67f83315c4c/apps/desktop/plans/20260817-remote-onboarding-improvements.md ; https://docs.superset.sh/llms.mdx/cli/host-server ; https://docs.superset.sh/llms.mdx/workspaces |
| Vibe Kanban | 페어링 코드 + PAKE | 코드는 6자 `A-Z0-9` 1회용이며 코드 자체에는 TTL이 없습니다. 교환은 SPAKE2로 하고 PAKE 창은 5분입니다. 시도는 60s당 5회로 제한합니다. 서비스는 종료 수순입니다. | 페어링된 클라이언트는 Ed25519 키로 식별하며 폐기할 수 있습니다. | https://github.com/BloopAI/vibe-kanban/blob/d5cbb5380fa0b32e98ef9b8d987f63decce4be3a/crates/trusted-key-auth/src/runtime.rs ; https://vibekanban.com/docs/remote-access.md ; https://www.vibekanban.com/blog/shutdown ; https://vibekanban.com/docs/workspaces/repositories.md |
| Conductor | Mac worktree 앱 | Mac 전용입니다. remote control은 QR을 찍고 Pair를 확인하는 방식이며 Revoke가 있습니다. 클라우드 sandbox도 제공합니다. | QR은 "works once and expires after 10 minutes"라고 되어 있지만 출처가 하나뿐이라 검증되지 않았습니다. | https://www.conductor.build/docs/reference/remote-control.md ; https://www.conductor.build/docs/concepts/git-worktrees.md |
| Crystal | Worktree 앱 | `git worktree list --porcelain`을 씁니다. deprecated입니다. | 해당 없음 | https://github.com/stravu/crystal/blob/1e18e0bc981225f75b5226f82a300fa741970c6f/main/src/services/worktreeManager.ts |
| Sculptor | Agent 앱 | 원격은 custom backend command로 지원합니다. | 해당 없음 | https://raw.githubusercontent.com/imbue-ai/sculptor/main/docs/help/experimental/container_backend.md |
| OpenAI Codex (cloud + remote connections) | 클라우드 agent / 원격 머신 | cloud에서는 secret을 setup 단계에만 넘기고 placeholder proxy를 씁니다. remote connection은 구체적인 `~/.ssh/config` alias를 쓰며, 원격에 `codex`가 미리 설치돼 있어야 합니다. | 폰 QR 페어링은 같은 계정과 MFA를 요구합니다. Revoke와 Offline은 별개입니다. | https://developers.openai.com/codex/cloud/environments.md ; https://learn.chatgpt.com/docs/remote-connections.md ; https://learn.chatgpt.com/docs/dots/computers-and-apps.md |
| Claude Code Remote Control / self-hosted envs | 아웃바운드 agent bridge | 연결은 아웃바운드 전용입니다. `Trust <dir>? [y/N]`를 묻고, home 디렉터리에 대해서는 답을 저장하지 않습니다. self-hosted environment secret은 한 번만 표시되고 365일 유효하며 `umask 077`로 저장됩니다. work-order JWT는 1회용이고 clock skew 5분을 허용합니다. | Trusted Devices는 18h 이내 로그인과 생체 인증을 요구합니다. `setup-token`은 쓸 수 없습니다. environment secret으로 대기 중인 세션을 가로챌 위험이 있습니다. | https://code.claude.com/docs/en/remote-control.md ; https://code.claude.com/docs/en/self-hosted-environments-quickstart.md ; https://code.claude.com/docs/en/self-hosted-environments-deploy.md ; https://code.claude.com/docs/en/claude-code-on-the-web.md |
| Cursor My Machines / Team Pools | self-hosted cloud-agent worker | `curl https://cursor.com/install -fsS \| bash`, `agent login`, `agent worker start --worker-dir` 순서로 설정합니다. 작업은 git remote 기준으로 라우팅되고 연결은 아웃바운드 전용입니다. | `/v1/sub-tokens`로 단기 token을 받습니다. `--session-token`은 1회 claim용이고 7일 backstop이 있습니다. 빌드는 서명을 검증합니다. | https://cursor.com/docs/cloud-agent/self-hosted/my-machines.md ; https://cursor.com/docs/cloud-agent/self-hosted/pool.md ; https://cursor.com/docs/cloud-agent/self-hosted.md |
| Devin Local Hands / Outposts | Agent worker | Local Hands는 `devin connect <CODE>`, Outposts는 `devin worker start --outpost --token`으로 붙입니다. `--force-manual-token-flow`를 지원합니다. | Local Hands 승인 카드에는 사유와 만료 시각이 나옵니다. 코드는 10분짜리 1회용이고 소유자 전용이며 새 요청이 이전 요청을 대체합니다. Outposts token은 한 번만 표시됩니다. partner 흐름은 PKCE S256과 10분 코드를 쓰고 atomic하게 소비합니다(`invalid_grant`, `no-store`). 두 번째 claim은 409입니다. | https://docs.devin.ai/work-with-devin/local-hands.md ; https://docs.devin.ai/cloud/outposts/quickstart.md ; https://docs.devin.ai/cloud/outposts/partners.md ; https://docs.devin.ai/cli/reference/commands.md |
| Google Jules | 클라우드 agent | GitHub로 연결하고 작업마다 VM을 쓰며 Run/Snapshot을 지원합니다. | GitHub app 연결로 인증합니다. | https://jules.google/docs/index.md ; https://jules.google/docs/environment/ |
| Wave Terminal (wsh) | 터미널 SSH helper 설치 | exec 한 번으로 `wsh version \|\| (echo not-installed; uname -sm); exec wsh connserver`를 실행하며, 원격 버전이 더 새로워도 받아들입니다. 번들 바이너리를 stdin으로 넘겨(`cat > temp; mv`) `~/.waveterm/bin/wsh`에 설치합니다. 연결 목록은 여러 소스를 정해진 순서로 병합합니다. 파일 브라우저는 수동 RPC를 쓰고 SFTP fallback이 없습니다(#2707). | "Install wsh / No wsh / auto"로 동의를 받으며 `askbeforewshinstall` 기본값은 true입니다. checksum은 없습니다. Ed25519 JWT를 stdin으로 보내고 기본 만료는 365일입니다. `AllowTcpForwarding`이 필요하고 세션 범위로만 동작합니다. host-key 모달은 SHA256이 아니라 base64로 보여 줍니다. | https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/conncontroller/conncontroller.go ; https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/connutil.go ; https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/wavejwt/wavejwt.go ; https://docs.waveterm.dev/connections ; https://github.com/wavetermdev/waveterm/issues/2707 |
| Warp SSH extension | 터미널 SSH helper 설치 | `ssh_extension_install_mode`는 `always_ask`/`always_install`/`never_install`이고 denylist가 있습니다. `~/.warp/remote-server`에 설치하며 glibc 2.31 이상이 필요합니다. app.warp.dev에서 curl/wget으로 받고, 안 되면 SCP로 옮겨 staged install합니다. | "Install … / Continue without"로 묻습니다. checksum은 없습니다. bearer는 프로토콜 메시지 안에서만 오가고 원격은 로그인하지 않습니다. keyring이 잠겨 있으면 막힙니다(#12449). | https://docs.warp.dev/terminal/warpify/ssh.md ; https://docs.warp.dev/terminal/settings/all-settings.md ; https://github.com/warpdotdev/warp/blob/1e6c3ae465d6a925a9b97ae03877aaadd159b8f1/crates/remote_server/src/install_remote_server.sh ; https://github.com/warpdotdev/warp/blob/1e6c3ae465d6a925a9b97ae03877aaadd159b8f1/crates/remote_server/src/auth.rs ; https://github.com/warpdotdev/warp/issues/12449 |
| Termius | SSH 클라이언트, 호스트 import | `~/.ssh`에서 import하지만 ProxyCommand는 무시합니다. Cloud Sync 그룹은 AWS ReadOnlyAccess로 import합니다. | vault는 SRP와 Argon2id로 E2E 암호화합니다. keychain을 못 쓰면 localStorage로 fallback합니다. | https://docs.termius.com/getting-started/import-existing-hosts.md ; https://docs.termius.com/security/encryption-overview.md ; https://docs.termius.com/organize-and-connect-to-hosts/groups-and-tags.md |
| Tabby | SSH 클라이언트, 호스트 import | OpenSSHImporter가 `Include`를 따라가고 mtime으로 캐시합니다. import된 프로필은 builtin이라 읽기 전용입니다. 기본 사용자는 root입니다(#7622). | vault는 PBKDF2 100k와 AES-CBC를 씁니다. sync는 bearer를 쓰고 HTTPS를 강제합니다. | https://github.com/Eugeny/tabby/blob/467ce8ebfa467fb88cf6d154c9a5891e3d3647f0/tabby-electron/src/sshImporters.ts ; https://github.com/Eugeny/tabby/issues/7622 ; https://github.com/Eugeny/tabby/blob/467ce8ebfa467fb88cf6d154c9a5891e3d3647f0/tabby-settings/src/services/configSync.service.ts |
| Royal TS Dynamic Folder | 스크립트 기반 호스트 인벤토리 | 스크립트가 RoyalJSON을 출력하면 읽기 전용 폴더로 보여 주고 다시 불러올 수 있습니다. | 확장된 스크립트를 temp 파일에 씁니다. | https://github.com/royalapplications/docs/blob/e19b9857e136da464988a0736e279aba34a844fa/src/products/royalts/reference/organization/dynamic-folder.md |
| WezTerm mux | SSH 위 터미널 mux | 원격에 `wezterm`이 미리 설치돼 있어야 하며 `wezterm cli --prefer-mux proxy`로 붙습니다. codec 버전이 정확히 일치해야 합니다(#1972는 stale mux 문제). `cli list --format json`이 pane cwd를 돌려줍니다. | `tlscreds`로 SSH를 통해 TLS를 bootstrap합니다. CA는 서버 시작마다 재생성되고 PEM은 평문으로 저장됩니다. | https://raw.githubusercontent.com/wezterm/wezterm/e2a98e1b8af156988ad0ac778658ac27b60a55fb/docs/multiplexing.md ; https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/wezterm-client/src/client.rs ; https://github.com/wezterm/wezterm/issues/1972 ; https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/wezterm-mux-server-impl/src/pki.rs |
| Eternal Terminal | SSH로 bootstrap하는 세션 서버 | 32자 passkey를 SSH stdin으로 보내고 `IDPASSKEY:` sentinel로 받습니다. `etserver` 서비스가 TCP 2022에서 돕니다. | 재연결은 challenge와 `resetProof`로 합니다. `~/.et/sessions`는 평문입니다. | https://github.com/MisterTea/EternalTerminal/blob/7ee361a75fbb3e0900445b110188996fbe950823/src/terminal/SshSetupHandler.cpp ; https://raw.githubusercontent.com/MisterTea/EternalTerminal/master/docs/protocol.md ; https://raw.githubusercontent.com/MisterTea/EternalTerminal/master/README.md |
| mosh | SSH로 bootstrap하는 UDP | 서버가 `MOSH CONNECT port key`를 출력하고, 60s 안에 클라이언트가 없으면 종료합니다. 서버 경로는 `--server=`로 지정합니다. | AES-128-OCB를 쓰며 key는 env로 전달합니다. | https://raw.githubusercontent.com/mobile-shell/mosh/master/man/mosh-server.1 ; https://mosh.org/ |
| tmux / zellij | 세션 multiplexer | tmux는 `/tmp/tmux-UID` socket과 파일 권한에 의존합니다. `ls -F` format, 안정적인 ID(`$ @ %`), control mode `-CC`의 `%sessions-changed`를 제공합니다. zellij web은 기본 127.0.0.1:8082이고 로그인을 4주간 기억합니다. | zellij web token은 한 번만 표시되고 해시로 저장되며, 폐기하면 연쇄 폐기됩니다. | https://raw.githubusercontent.com/tmux/tmux/master/tmux.1 ; https://raw.githubusercontent.com/wiki/tmux/tmux/Control-Mode.md ; https://zellij.dev/documentation/web-client.html |
| sshx | 웹 터미널 공유 | URL fragment에 14자(83 bits) key를 넣습니다. 쓰기 password는 따로 둡니다. 300s 유휴 시 퇴출되며 그 외 TTL은 없습니다. | 서버는 `encrypted_zeros` verifier만 갖고 `ct_eq`로 비교합니다. KDF는 Argon2id입니다. | https://github.com/ekzhang/sshx/blob/3604e8ec5ce0e741bd4a7de2e339b980ca8a2d40/crates/sshx/src/controller.rs ; https://github.com/ekzhang/sshx/blob/3604e8ec5ce0e741bd4a7de2e339b980ca8a2d40/crates/sshx-server/src/web/socket.rs ; https://github.com/ekzhang/sshx/issues/146 |
| upterm | relay 터미널 공유 | `--authorized-user github:`와 `--join-timeout`을 지원합니다. 처음 들어온 게스트가 세션을 claim합니다. | relay는 `@cert-authority`로 pin하고, 인증서는 ±1분짜리로 발급합니다. | https://github.com/owenthereal/upterm/blob/845c706ec0735459ea0d4b0b04df211f7e1a86df/README.md ; https://github.com/owenthereal/upterm/blob/845c706ec0735459ea0d4b0b04df211f7e1a86df/server/cert.go ; https://github.com/owenthereal/upterm/blob/845c706ec0735459ea0d4b0b04df211f7e1a86df/docs/upterm_host.md |
| tmate / action-tmate | relay 터미널 공유 | token은 55자 알파벳으로 된 25자(약 144.5 bits)이고 읽기 전용 `ro-` token이 따로 있습니다. `-a`는 authorized keys, `-k`는 API key, `-n`은 세션 이름입니다. 공개 서버는 2025년 7월에 종료됐습니다. action-tmate는 `limit-access-to-actor`를 지원합니다. | `-n`으로 이름을 붙인 세션은 추측할 수 있습니다. | https://github.com/tmate-io/tmate-ssh-server/blob/d7334ee4c3c8036c27fb35c7a24df3a88a15676b/tmate-ssh-daemon.c ; https://web.archive.org/web/2024id_/https://tmate.io/ ; https://github.com/tmate-io/tmate/issues/336 ; https://github.com/mxschmitt/action-tmate/blob/master/README.md |
| Tailscale auto-update | agent 자가 업데이트 | 약 1주 holdback을 둡니다. `tailscale set --auto-update`로 켜고, 수동으로는 `tailscale update --track --version --dry-run`을 씁니다. minor 버전이 짝수면 stable입니다. MDM `InstallUpdates` 정책으로 제어할 수 있습니다. | distsign으로 검증합니다. 바이너리에 내장된 Ed25519 root, BLAKE2s, 길이를 확인합니다. 다운로드는 `.unverified`로 받고 검증한 뒤 rename합니다. | https://tailscale.com/docs/features/client/update.md ; https://tailscale.com/docs/reference/tailscale-cli.md ; https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/clientupdate/distsign/distsign.go ; https://tailscale.com/docs/features/tailscale-system-policies.md |
| Teleport Managed Updates v2 | agent 자가 업데이트 | `teleport-update enable`이 systemd timer를 등록합니다. 버전은 `/opt/teleport/default/versions/X.Y.Z`에 두고 symlink로 전환하며 pin/unpin할 수 있습니다. 업데이트는 월~목 16:00 UTC 창에 60s jitter를 주고 canary와 rollback을 거칩니다. | agent는 클러스터보다 새로울 수 없고 1 major까지 뒤처지는 것은 허용됩니다. RFD 0184는 패키지 매니저를 쓰지 않고, 서명은 범위에서 뺐습니다. | https://goteleport.com/docs/upgrading/agent-managed-updates.md ; https://goteleport.com/docs/reference/cli/teleport-update.md ; https://goteleport.com/docs/upgrading/overview.md ; https://github.com/gravitational/teleport/blob/1283425b60ec5f60d509ba4c791183d452923ff7/rfd/0184-agent-auto-updates.md |
| VS Code server cache / update | 서버 버전 관리 | `{quality}-{commit}` 디렉터리를 나란히 둡니다(side-by-side). 정지된 서버만 prune하고, 업데이트는 알림으로 알려 줍니다. | commit에 고정됩니다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/paths.rs ; https://code.visualstudio.com/docs/remote/vscode-server.md |
| VibeTunnel (설치·웹 인증) | 브라우저 터미널 서버 | macOS는 `brew install --cask vibetunnel`, Linux는 `npm install -g vibetunnel` 뒤 `vibetunnel systemd`로 설치합니다. 후자는 `~/.config/systemd/user/vibetunnel.service`를 만들고 `loginctl enable-linger $USER`를 실행합니다. 기본값은 `--port` 4020, `--bind` `0.0.0.0`, Restart 10s, MemoryHigh 512MB / MemoryMax 1GB이며 root로는 실행할 수 없습니다. 인증 플래그는 `--enable-ssh-keys`, `--disallow-user-password`, `--no-auth`, `--allow-local-bypass [--local-auth-token]`입니다. SSH 설치나 계정 enrollment는 없습니다. | 기본 인증은 PAM입니다. 로그인하면 HS256 JWT `{userId, iat}`를 `expiresIn: '24h'`로 발급하며 refresh, scope, 서버 측 revocation이 없습니다. secret은 `~/.vibetunnel/jwt-secret`(0600, temp+`link`로 atomic 생성)에 둡니다. SSH 키 로그인은 32-byte challenge(TTL 5분, 성공 시 삭제, `ssh-ed25519`만)를 쓰지만 `authorized_keys`를 substring `includes`로 확인합니다. 클라이언트는 JWT를 `localStorage`에 두고, 서버는 `?token=` query도 받습니다. 내장 TLS가 없습니다. | https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/auth-service.ts ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/authentication.md ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/docs/systemd.md ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/client/services/auth-client.ts ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/security.md |
| VibeTunnel HQ mode | 다중 머신 등록 | 원격 머신은 `--hq-url https://… --hq-username … --hq-password … --name <unique> [--remote-url]`로 띄웁니다. 그러면 HTTP Basic으로 `POST /api/remotes/register`에 `{id: uuidv4, name, url, token}`을 보냅니다. HQ는 `GET /api/health`를 15s 간격, 5s timeout으로 확인하고 3회 연속 실패하면 퇴출합니다. 원격은 60s마다 재등록합니다. 같은 id/name/url/token이면 204로 idempotent하게 갱신합니다. registry는 메모리 `Map`에만 있습니다. `--allow-insecure-hq`가 없으면 HTTPS를 강제합니다. HQ 모드에서는 repo discovery가 숨겨져 경로를 직접 입력해야 합니다. | 모든 원격이 같은 HQ Basic 자격 증명을 공유하고 재사용합니다. 이 값은 CLI 플래그로 넘어가므로 `ps`와 unit 파일에 그대로 드러납니다. 원격 bearer는 기동마다 새로 만드는 UUIDv4이고 원격에서 명령을 실행할 수 있는 전권을 가집니다. 승인 단계는 없습니다. 같은 name을 다른 id로 등록하는 것만 거부합니다. | https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/hq.md ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/remote-registry.ts ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/hq-client.ts ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/server.ts |
| VibeTunnel 원격 접근·discovery | Tailscale/터널 + mDNS + repo 탐색 | 원격 접근 방법은 6가지입니다. Tailscale(직접 연결, Serve HTTPS, Funnel ACL `"attr": ["funnel"]`), ngrok(authtoken은 Keychain `vibeTunnelService`; `ngrok http <port> --log=stdout --log-format=json`, URL 대기 10s), LAN, `cloudflared tunnel --url http://localhost:4020`, Pinggy, Pangolin/Newt입니다. iOS 앱은 Bonjour `_vibetunnel._tcp`(기본 광고, `--no-mdns`로 끔)와 Tailscale API로 머신을 찾고, `/api/health`를 3s timeout으로 probe하며 30s마다 갱신합니다. repo는 `GET /api/repositories/discover?path=&maxDepth=`로 찾습니다. 기본 경로는 `~/Documents`, depth 3이고 hidden 디렉터리는 건너뛰며 `.git`(파일 또는 디렉터리)에서 멈춥니다. branch 조회는 `git worktree list --porcelain`으로 `worktreePath`를 붙입니다. | Tailscale identity header는 loopback에서 온 요청이고 x-forwarded-* 헤더가 모두 있을 때만 믿습니다. Tailscale 공식 문서도 backend를 localhost로만 열라고 권합니다. `npx -y vibetunnel --no-auth --ngrok`는 인증 없는 shell을 공개합니다. ngrok token은 argv에 노출됩니다. `/api/health`는 인증 없이 version, pid, Tailscale 상태를 노출합니다. #539/#556은 Tailscale 단계가 안내되지 않거나 오류 메시지가 없어 생긴 온보딩 실패 사례입니다. | https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/README.md ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/middleware/auth.ts ; https://tailscale.com/docs/features/tailscale-serve.md ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/mac/VibeTunnel/Core/Services/NgrokService.swift ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/ios/VibeTunnel/Services/BonjourDiscoveryService.swift ; https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/routes/repositories.ts ; https://github.com/amantus-ai/vibetunnel/issues/539 ; https://github.com/amantus-ai/vibetunnel/issues/556 |

### 반복 패턴

- **SSH 자동 설치는 대부분 같은 순서입니다.** 원격에서 먼저 다운로드하고, 실패하면 업로드한 뒤, atomic move로 마무리합니다. VS Code, Zed, JetBrains, Warp, Wave, DevPod, Orca, Superset이 모두 이 흐름입니다. checksum은 거의 검증하지 않습니다. 예를 들어 VS Code는 `x-sha256`을 무시합니다. 검증하는 쪽은 Tailscale distsign과 Cursor worker 정도입니다.
  - https://github.com/jeanp413/open-remote-ssh/blob/1911064579e9b6162ee90bcd20bf8d1b9d5af552/src/scripts/server-setup.sh
  - https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/remote/src/transport/ssh.rs#L1146-L1330
  - https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/clientupdate/distsign/distsign.go
- **commit/version별로 나란히 두는 설치 디렉터리와 버전 협상이 흔합니다.** 디렉터리는 VS Code `code-<commit>`, Zed `<channel>-<version>`, Orca `orcad-<ver>`, Teleport `versions/X.Y.Z`+symlink 형태입니다. 버전 규칙은 제품마다 다릅니다. WezTerm과 Zed는 정확히 일치해야 하고, Coder는 major.minor로 degrade하며, Teleport는 agent가 클러스터보다 앞서는 것을 금지합니다.
  - https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/paths.rs
  - https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/cli/ping.go
  - https://goteleport.com/docs/upgrading/overview.md
- **환경 확인은 exec 한 번에 sentinel 줄로 받습니다.** Wave(`wsh version || (echo not-installed; uname -sm)`), ET(`IDPASSKEY:`), mosh(`MOSH CONNECT`), DevPod(ping/pong), Superset(glibc/musl, `/etc/machine-id`)이 그렇습니다.
  - https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/conncontroller/conncontroller.go
  - https://github.com/MisterTea/EternalTerminal/blob/7ee361a75fbb3e0900445b110188996fbe950823/src/terminal/SshSetupHandler.cpp
  - https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/packages/cli/src/commands/hosts/add/command.ts
- **비밀은 argv가 아니라 stdin으로 보내는 것이 다수입니다.** Superset `--api-key=-`, Wave JWT, ET passkey, DevPod 바이너리가 이 방식입니다. 반대 사례는 mosh(env), VibeTunnel(HQ password를 CLI 플래그로, ngrok token을 argv로)입니다.
  - https://raw.githubusercontent.com/mobile-shell/mosh/master/man/mosh-server.1
  - https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/hq.md
- **상주 서비스로 설치하는 쪽과 세션 범위 helper로 끝내는 쪽으로 갈립니다.**
  - 서비스 설치: VS Code Tunnels(systemd user unit, Aqua, HKCU Run), Superset(`KillMode=process`, linger 질문), VibeTunnel(`vibetunnel systemd` + `loginctl enable-linger`), Teleport timer.
  - 세션 범위: Zed, Wave, Orca, Coder external, DevPod.
  - https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/service_linux.rs
  - https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/docs/systemd.md
- **SSH가 없는 머신은 짧은 1회용 코드로 등록합니다.** Devin(10분, 소유자 전용, 대체됨, 두 번째 claim 409), Superset 계획(10~15분, POST로만 redeem), Conductor(1회, 10분), Vibe Kanban(PAKE 5분, 60s당 5회), Cursor `--session-token`(1회 claim)이 예입니다.
  - https://docs.devin.ai/work-with-devin/local-hands.md
  - https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/plans/20261010-add-host-over-ssh.md
  - https://cursor.com/docs/cloud-agent/self-hosted/my-machines.md
- **약하거나 재사용되는 페어링은 피해야 할 패턴입니다.** Happy는 poll마다 token을 새로 발급하고 TTL이 없습니다. Orca는 첫 사용 뒤 만료 없는 grant가 됩니다. Coder agent token은 만료되지 않습니다. Wave JWT는 365일입니다. VibeTunnel HQ는 모든 머신이 같은 Basic 자격 증명을 쓰고 승인 단계가 없습니다.
  - https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-server/sources/app/api/routes/authRoutes.ts
  - https://raw.githubusercontent.com/coder/coder/main/coderd/httpmw/workspaceagent.go
  - https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/remote-registry.ts
- **host key 처리는 엄격한 쪽과 느슨한 쪽이 뚜렷이 나뉩니다.**
  - 엄격: Orca TOFU(자격 증명 전송 전에 실패), JetBrains(`fp` 필수), upterm(`@cert-authority`).
  - 느슨: DevPod과 gh(`StrictHostKeyChecking no`), Wave(base64로 표시).
  - https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/ssh/ssh-host-key-store.ts
  - https://github.com/loft-sh/devpod-provider-ssh/blob/master/pkg/ssh/ssh.go
- **repo/worktree discovery는 범위를 제한한 스캔과 `git worktree list --porcelain` 조합이 주류입니다.** Orca는 BFS depth 3, 최대 100, timeout, `.gitignore`를 따릅니다. VibeTunnel은 `~/Documents` depth 3, hidden 스킵, `.git`에서 멈춤입니다. Crystal과 Happy는 porcelain을 쓰는데 Happy는 flood 문제(#1736)가 있습니다. 머신을 넘나드는 discovery는 VibeTunnel HQ에서도 구현되지 않았습니다.
  - https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/project-groups/nested-repo-discovery.ts
  - https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/routes/repositories.ts
  - https://github.com/slopus/happy/issues/1736
- **연결은 아웃바운드 전용과 loopback 바인딩이 표준입니다.** VS Code Tunnels, Claude Code Remote Control, Cursor worker, Superset(127.0.0.1+relay)이 그렇습니다. VibeTunnel의 Tailscale Serve 모드도 loopback을 강제합니다.
  - https://code.claude.com/docs/en/remote-control.md
  - https://docs.superset.sh/llms.mdx/remote-access
  - https://tailscale.com/docs/features/tailscale-serve.md
- **안전장치가 있는 자가 업데이트가 쓰입니다.** Tailscale(holdback, 서명), Teleport(창, jitter, canary, rollback), Superset(매시간, rollback)이 예입니다.
  - https://tailscale.com/docs/features/client/update.md
  - https://goteleport.com/docs/upgrading/agent-managed-updates.md

### 상충되는 주장

- **VS Code 서버 경로.** 구버전 기준 자료는 `~/.vscode-server/bin/<commit>`입니다(https://github.com/jeanp413/open-remote-ssh/blob/1911064579e9b6162ee90bcd20bf8d1b9d5af552/src/scripts/server-setup.sh). 최신 이슈는 `code-<commit>`과 `cli/servers/Stable-<commit>`입니다(https://github.com/microsoft/vscode-remote-release/issues/11136 , https://github.com/microsoft/vscode-remote-release/issues/11721). **판정:** 최신 layout이 현행입니다. Cursor의 `cli/servers/Stable-<commit>`도 이와 일치합니다.
- **Wave의 시스템 ssh_config 경로.** 문서는 `/etc/ssh/ssh_config`라고 합니다(https://docs.waveterm.dev/connections). 코드는 `/etc/ssh/config`를 읽습니다(https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/connutil.go). **판정:** 실제 동작은 코드를 따르고, 버그일 가능성이 높습니다.
- **tmate token 엔트로피.** 아카이브된 사이트는 150 bits라고 합니다(https://web.archive.org/web/2024id_/https://tmate.io/). 소스상으로는 약 144.5 bits입니다(https://github.com/tmate-io/tmate-ssh-server/blob/d7334ee4c3c8036c27fb35c7a24df3a88a15676b/tmate-ssh-daemon.c). **판정:** 소스가 맞습니다.
- **DevPod 비활성 timeout.** 문서의 `INACTIVITY_TIMEOUT`(https://devpod.sh/docs/developing-in-workspaces/inactivity-timeout)과 이슈에 보고된 동작(https://github.com/loft-sh/devpod/issues/776)이 다릅니다. **미해결**입니다.
- **VS Code 10-tunnel 한도.** 통념은 가장 오래된 터널을 지운다는 것입니다. 코드는 유휴 터널을 *무작위로* 지웁니다(https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/dev_tunnels.rs). **판정:** 코드 기준으로 무작위입니다.
- **Conductor QR 10분 TTL.** 근거가 https://www.conductor.build/docs/reference/remote-control.md 하나뿐이고 교차 확인은 하지 않았습니다. **미검증**입니다.
- **Remote-SSH Alpine 지원.** 문서는 "not yet supported"라고 합니다(https://code.visualstudio.com/docs/remote/linux). 반면 update_service.rs에는 `server-linux-alpine` artifact가 있다는 상반된 근거가 있습니다. 이 파일의 URL은 입력 자료에 없습니다. **미해결**이지만 영향은 작습니다.
- **VibeTunnel 인증 문서와 코드.** stale 문서인 `docs/features/authentication.md`는 Bun.serve, JWT `scope`, localhost 기본 바인딩을 설명합니다(입력 자료에 URL 없음). 소스는 Express, scope 없음, CLI 기본 `0.0.0.0`입니다(https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/auth-service.ts , https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/authentication.md). **판정:** 소스 코드를 따릅니다.
- **VibeTunnel Tailscale Serve 토글.** 사용자들은 Serve를 켜면 원격 접속이 된다고 기대했습니다. 실제로는 Serve를 켜면 127.0.0.1에 바인딩되어 직접 Tailscale 접속이 끊기고, 꺼야만 접속됐습니다(https://github.com/amantus-ai/vibetunnel/issues/556). 이는 Tailscale의 localhost 권고(https://tailscale.com/docs/features/tailscale-serve.md)와 같은 원리에서 나옵니다. **판정:** 의도된 동작이지만 UX가 안내하지 못한 문제입니다. 2026-06-12에 close됐습니다.

## 2.3 디스커버리·서비스 등록

| 제품 | 분류 | 방식(명령/기본값) | 보안·프라이버시 | 출처 |
|---|---|---|---|---|
| VS Code recently-opened list | IDE 최근 항목 | `history.recentlyOpenedPathsList`를 global storage(`state.vscdb` SQLite)에 저장한다. 최대 500개까지 MACHINE scope로 보관한다. 항목마다 `remoteAuthority`를 기록하므로 원격 폴더와 로컬 폴더가 따로 관리된다. | 로컬 사용자 프로필 파일에만 저장되며 프롬프트는 없다. 원격 호스트 경로는 authority와 함께 저장된다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/platform/storage/electron-main/storageMain.ts |
| VS Code Git extension repo scan | IDE repo 탐지 | 기본값은 `git.autoRepositoryDetection` `true`, `git.repositoryScanMaxDepth` 1, `git.repositoryScanIgnoredFolders` `["node_modules"]`, `git.openRepositoryInParentFolders` `prompt`이다. `git.detectWorktrees`는 기본 false이고 상한은 50이다. 상한에 도달하면 사용자에게 알리지 않고 스캔을 멈춘다(issue 341361). | dubious ownership 검사에 걸린 repo는 `UnsafeRepositoriesManager`로 넘어가며, 사용자가 직접 opt-in해야 한다. 스캔은 workspace trust가 있어야 실행된다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/src/model.ts ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/package.json#L3305-L3324 ; https://github.com/microsoft/vscode/issues/341361 |
| VS Code SimpleFileDialog | 원격 파일 선택기 | Quick Pick 기반 경로 탐색기로, 원격 file system provider가 내용을 제공한다. 기본 경로(원격 home 또는 workspace)에서 시작하며, scheme이 local이 아니면 이 선택기를 쓴다. | 원격 사용자가 읽을 수 있는 곳은 모두 탐색할 수 있다. 별도 allowlist는 없다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/browser/simpleFileDialog.ts ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/browser/abstractFileDialogService.ts ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/electron-browser/fileDialogService.ts#L67-L74 |
| VS Code Remote-SSH / VS Code Server | 원격 개발 부트스트랩 | SSH로 서버를 설치하고, 서버는 무작위 connection key로 보호된다. `/version`은 인증 없이 응답한다. TCP 포트 대신 socket으로 listen하는 옵션이 있다. | 신뢰는 SSH에서 오고, 서버 포트는 무작위 key가 추가로 보호한다. | https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/ssh.md ; https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/troubleshooting.md |
| VS Code Tunnels | 원격 접속·서비스 설치 | GitHub/Microsoft device flow로 로그인한다. `auth.rs`는 device code를 5초마다 polling하고, token을 keyring에 저장하되 실패하면 0600 권한의 `token.json`에 저장한다. 계정당 tunnel은 10개까지이며, 초과하면 무작위로 하나를 내보낸다. 서비스는 `code tunnel service install`과 `code tunnel unregister`로 관리한다. | 계정이 신뢰의 기준이다. keyring을 쓸 수 없으면 0600 파일로 대체한다. | https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/tunnels.md ; https://raw.githubusercontent.com/microsoft/vscode-docs/main/docs/remote/tunnels.md ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/auth.rs |
| VS Code tunnel service on Linux | Linux 서비스 | session D-Bus를 통해 systemd user unit을 설치하고, linger를 켜라는 팁만 출력할 뿐 직접 켜지는 않는다. systemd만 지원한다. headless 세션에서는 D-Bus 오류가 난다. linger가 없으면 로그아웃 시 tunnel이 멈추며, 메인테이너는 이를 정상 동작으로 본다. | user unit이므로 root가 필요 없다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_linux.rs ; https://github.com/microsoft/vscode-remote-release/issues/7778 ; https://github.com/microsoft/vscode/issues/246907 ; https://github.com/microsoft/vscode/issues/173558 |
| VS Code tunnel service on macOS | macOS 서비스 | `$HOME`에 Aqua 전용 LaunchAgent plist를 쓰고, 구식 `launchctl load`로 로드한다. | 사용자 수준이다. 아무도 로그인하지 않은 상태에서는 실행되지 않는다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_macos.rs |
| VS Code tunnel service on Windows | Windows 서비스 | 실제 Windows 서비스가 아니라 HKCU `Run` 레지스트리 키를 쓰므로 로그온할 때만 시작된다. 시작에 실패한다는 사용자 보고가 있다. | 관리자 권한이 필요 없다. | https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_windows.rs ; https://github.com/microsoft/vscode/issues/167741 ; https://github.com/microsoft/vscode/issues/184058 |
| Project Manager (alefragnani) | VS Code 확장 repo 탐색기 | `baseFolders`로 루트를 지정하고, `maxDepthRecursion`은 기본 4이며, 무시할 폴더 목록이 있다. `.git/config`나 `gitdir:` 파일이 있으면 repo로 판정하고 결과를 캐시한다. 원격 동작은 `remote.extensionKind`로 설정한다. | 사용자가 설정한 루트만 스캔한다. | https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/package.json ; https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/src/autodetect/gitRepositoryDetector.ts ; https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/README.md |
| Zed remote development | 원격 개발·최근 항목 | `ssh`를 호출해 연결한다. 버전이 고정된 `~/.zed_server`를 설치하며, 바이너리를 SSH로 업로드하는 옵션이 있다. 호스트 목록은 `ssh_config`에서 읽는다. 원격 picker는 home에서 열리고 스캔은 하지 않는다. 최근 항목은 SQLite에 저장되며, worktree `identity_paths`로 중복을 제거하고 오래된 항목은 7일 후 GC한다. | worktree trust는 호스트별로 부여된다. | https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/docs/src/remote-development.md ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/assets/settings/default.json ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/recent_projects/src/remote_servers.rs ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/workspace/src/persistence.rs#L2723-L2775 ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/workspace/src/persistence.rs#L2159-L2213 ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/docs/src/worktree-trust.md |
| Zed open-path prompt | 원격 파일 선택기 | 입력하는 동안 디렉터리와 suffix를 자동 완성하는 경로 picker다. | 기존 원격 연결을 통해서만 읽는다. | https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/open_path_prompt/src/open_path_prompt.rs#L71 |
| JetBrains recent projects | IDE 최근 항목 | `recentProjects.xml`에 저장되며 settings roaming 대상에서 빠진다. 최대 50개이고, 이미 열려 있는 프로젝트는 건너뛴다. | 로컬에만 저장된다. | https://github.com/JetBrains/intellij-community/blob/d8c6714c7a3c110c56994d6b0f10814f902100e7/platform/platform-impl/src/com/intellij/ide/RecentProjectsManagerBase.kt ; https://www.jetbrains.com/help/idea/advanced-settings.html |
| JetBrains Gateway / Remote Dev | 원격 개발 부트스트랩 | SSH로 연결하고 사용자가 프로젝트 디렉터리를 고르면, IDE backend를 `~/.cache/JetBrains/RemoteDev/dist`에 설치한다. join link에는 `jt=`(UUID)와 fingerprint가 들어 있다. | join link 자체가 자격 증명이고, fingerprint로 호스트를 고정한다. | https://www.jetbrains.com/help/idea/remote-development-a.html |
| GitHub Desktop | Git GUI repo 추가 | `git rev-parse --is-bare-repository --show-cdup --git-dir` 한 번으로 경로를 판정하고, `git worktree list --porcelain -z`로 worktree를 나열한다. 재귀 스캔은 하지 않으며, 사용자가 repo를 하나씩 추가한다. | 소유권이 안전하지 않은 repo는 대화상자를 띄워 `safe.directory`에 추가할지 묻는다. | https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/lib/git/rev-parse.ts ; https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/ui/add-repository/add-existing-repository.tsx ; https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/lib/git/worktree.ts ; https://github.com/desktop/desktop/issues/1574 ; https://github.com/desktop/desktop/issues/19662 ; https://github.com/desktop/desktop/issues/969 ; https://github.com/desktop/desktop/issues/21260 |
| Fork | Git GUI repo 스캔 | "Scan for repositories"는 폴더를 깊이 6(메인테이너 설명) 또는 5(이후 사용자 보고)까지 탐색한다. 깊이는 `repositories.toml`의 `scan_depth`로 조정한다. `~`에서 스캔했다가 100 GB로 부풀어 오른 사례가 있다. | home 디렉터리 스캔을 막는 장치가 없다. | https://github.com/fork-dev/Tracker/issues/2067 ; https://github.com/fork-dev/Tracker/issues/640 ; https://github.com/fork-dev/Tracker/issues/2512 ; https://git-fork.com/releasenotes ; https://github.com/fork-dev/Tracker/issues/1094 |
| Sourcetree | Git GUI repo 스캔 | 깊이 제어 없이 디렉터리를 스캔한다. 상위 경로의 `.git`을 잡아내는 것이 알려진 위험이다. | repo마다 자격 증명을 한 번씩 묻는다. | https://jira.atlassian.com/browse/SRCTREE-3001 ; https://jira.atlassian.com/rest/api/2/issue/SRCTREE-5100?fields=summary,description,comment,status,created ; https://jira.atlassian.com/rest/api/2/issue/SRCTREE-2697?fields=summary,description,comment,status,resolution,created ; https://jira.atlassian.com/rest/api/2/issue/SRCTREE-6860?fields=summary,description,comment,status,created ; https://jira.atlassian.com/rest/api/2/issue/SRCTREE-8009?fields=summary,description,status,created |
| GitKraken Desktop | Git GUI workspace | Local Workspace의 "Sync with local directory"가 사용자가 고른 폴더 안의 repo를 가져온다. | 폴더는 사용자가 명시적으로 고른다. | https://help.gitkraken.com/gitkraken-desktop/workspaces/ ; https://help.gitkraken.com/gitkraken-desktop/open-clone-init/ |
| ghq | CLI repo 탐색기 | 루트는 `~/ghq`이고 `GHQ_ROOT`로 바꿀 수 있다. 탐색 중 VCS 마커를 찾으면 `SkipDir`를 반환해 그 아래로 내려가지 않는다. worktree는 `.git/worktrees`로 찾는다. bare repo는 탐지하지 못한다. | 관리되는 단일 루트만 다룬다. | https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/README.adoc ; https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/local_repository.go ; https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/worktree.go |
| tmux-sessionizer | CLI repo 탐색기 | 기본적으로 `find <paths> -mindepth 1 -maxdepth 1 -type d`를 실행한다. 선택한 디렉터리의 `.tmux-sessionizer` 파일을 source하며, 현재 디렉터리에서 config를 불러올 수도 있다. | 디렉터리별 파일을 source하므로 해당 디렉터리의 코드가 실행된다. | https://github.com/ThePrimeagen/tmux-sessionizer/blob/7edf8211e36368c29ffc0d2c6d5d2d350b4d729b/tmux-sessionizer |
| sesh | CLI 세션 탐색기 | 여러 소스(tmux, config, zoxide)를 정해진 순서로 합친다. caching lister는 재검증하는 동안 이전 결과를 제공하며(stale-while-revalidate), soft TTL은 5초다. worktree에는 별도의 이름 규칙이 있다. | 로컬에서만 동작한다. | https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/lister/caching_lister.go ; https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/lister/list.go ; https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/README.md ; https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/namer/git.go |
| zoxide | frecency 디렉터리 이력 | frecency로 순위를 매긴다. 점수 가중치는 1시간 이내 x4, 1일 이내 x2, 1주 이내 x0.5, 그 이상은 x0.25다. `_ZO_MAXAGE` 기본값은 10000이다. 사라진 디렉터리는 90일 후 정리하고, `$HOME`은 제외한다. 데이터는 `db` 파일에 저장한다. | 로컬에서만 동작하며, 사용 기록에서 수동적으로 학습한다. | https://github.com/ajeetdsouza/zoxide/blob/a1b102b5a5e43e854a81ab754548fec93a2c1679/src/db/dir.rs ; https://github.com/ajeetdsouza/zoxide/blob/a1b102b5a5e43e854a81ab754548fec93a2c1679/src/db/stream.rs ; https://github.com/ajeetdsouza/zoxide/blob/a1b102b5a5e43e854a81ab754548fec93a2c1679/README.md ; https://github.com/ajeetdsouza/zoxide/wiki/Algorithm |
| Projectile (Emacs) | 에디터 프로젝트 탐색기 | `projectile-project-search-path`에 `(DIR . DEPTH)` 항목을 넣는다. 탐색은 세션당 한 번, 필요할 때(lazy) 실행된다. 원격(TRAMP) 경로는 건너뛴다. worktree 간 전환을 지원한다. | 사용자가 설정한 루트만 스캔한다. | https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/usage.adoc ; https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/projects.adoc ; https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/configuration.adoc ; https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/across_repositories.adoc |
| fzf-git.sh | CLI git picker | branch, worktree 등 git 객체를 고르는 fzf picker다. `git worktree list` 같은 git 명령으로 목록을 만든다. | 로컬에서만 동작한다. | https://github.com/junegunn/fzf-git.sh/blob/57ef995e245923ae05721e876c42100a1896ad6c/fzf-git.sh |
| Git repository layout / worktrees | Git 구조 판정 | linked worktree에는 `gitdir:`가 담긴 `.git` *파일*이 있고, 관리 디렉터리는 `.git/worktrees/<id>`에 있다. `git worktree list --porcelain [-z]`는 안정적인 출력을 낸다. 경로 판정에는 `git rev-parse --show-toplevel`, `--git-dir`, `--git-common-dir`, `--is-bare-repository`를 쓴다. 환경 변수 `GIT_DIR`과 `GIT_CEILING_DIRECTORIES`가 탐색 범위를 제한한다. | 탐색은 `GIT_CEILING_DIRECTORIES`에서 멈춘다. | https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/gitrepository-layout.adoc ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git-worktree.adoc ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/builtin/worktree.c ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git-rev-parse.adoc ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git.adoc |
| Git `safe.directory` / `safe.bareRepository` | Git 보안 | 2.30.3(CVE-2022-24765)부터 git은 다른 사용자가 소유한 repo를 `safe.directory`에 등록되지 않은 한 사용하지 않는다. Git 3.0부터 `safe.bareRepository=explicit`이 기본값이 된다. | "dubious ownership" 오류가 여기서 나온다. 이 오류는 숨기지 말고 사용자에게 보여 줘야 한다. | https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/config/safe.adoc ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/RelNotes/2.30.3.adoc |
| walkdir / ignore crate / fd | Rust/CLI walker | walkdir는 `max_depth`, `follow_links(false)`, 하위 트리를 잘라 내는 `filter_entry`를 지원한다. `ignore` crate의 `WalkBuilder`는 `.gitignore`/`.ignore`를 따르고, 숨김 파일을 건너뛰며, `max_depth`와 병렬 탐색을 지원한다. fd도 같은 기본값을 쓴다. | symlink 추적은 기본적으로 꺼져 있다. | https://github.com/BurntSushi/walkdir/blob/6fd031c82ba5a4204b4ce6eae73dacb00dc072ec/src/lib.rs ; https://github.com/BurntSushi/ripgrep/blob/3fce3b5bb0236da2df6d99672afb8a719642eca7/crates/ignore/src/walk.rs ; https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html ; https://github.com/sharkdp/fd/blob/9e904639aee44df4507200cf0ed5e8531e120d6f/README.md |
| Claude Code project history | 에이전트 이력 탐색 | transcript는 `~/.claude/projects/<encoded-cwd>/*.jsonl`에 있고, `cleanupPeriodDays` 기본값은 30이다. 폴더명 인코딩은 영숫자가 아닌 문자를 모두 `-`로 바꾸므로 원래 경로를 복원할 수 없다. 실제 경로는 JSONL 안의 `cwd` 필드에서 읽어야 한다. | 대화 내용이 담긴 로컬 파일이다. | https://code.claude.com/docs/en/claude-directory.md ; https://github.com/ryoppippi/ccusage/blob/06347d39a22a1ef61c022e20b22afade702a31d5/rust/adapters/claude/src/paths.rs ; https://github.com/d-kimuson/claude-code-viewer/blob/9367cf006be9965c954e6cd36526f2195dce67ea/src/server/core/sync/services/SyncService.ts |
| Codex CLI rollouts | 에이전트 이력 탐색 | rollout은 `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`에 있다. lister는 파일마다 앞부분 레코드를 최대 10개(`HEAD_RECORD_LIMIT`)만 읽고, 최대 10000개 파일(`MAX_SCAN_FILES`)까지만 스캔한다. `cwd`는 세션 메타데이터에서 얻는다. | 대화 내용이 담긴 로컬 파일이다. | https://github.com/openai/codex/blob/f3a90545aed3188ee1e5c119ec89a742de399cdc/codex-rs/rollout/src/list.rs ; https://developers.openai.com/codex/config-advanced.md ; https://github.com/ryoppippi/ccusage/blob/06347d39a22a1ef61c022e20b22afade702a31d5/rust/adapters/codex/src/paths.rs |
| Raycast "Cursor Recent Projects" | 최근 항목 리더 | Cursor/VS Code의 `state.vscdb` SQLite 파일을 직접 읽어 최근 프로젝트를 나열한다. | 다른 앱의 내부 저장소를 읽는다. | https://github.com/raycast/extensions/blob/26ee20de2d66c9d8b18225d9fc4211a2f0689387/extensions/cursor-recent-projects/src/db.ts |
| Happy (slopus) | 모바일 에이전트 세션 위치 | 새 세션 picker가 머신의 최근 세션 경로와 machine choice로 위치 목록을 만든다. `pathSecurity`가 모든 경로가 허용된 루트 아래에 있는지 확인한다. | CLI 쪽에서 경로 포함 여부를 검사하고 검증한다. | https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-ios/Sources/HappyCore/NewSession/SessionPlaces.swift ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-app/sources/sync/machineChoices.ts ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/modules/common/pathSecurity.ts |
| macOS TCC (Documents/Desktop/Downloads, Full Disk Access) | OS 프라이버시 | 보호 폴더를 읽으면 TCC 프롬프트가 뜨며, 이를 위해 `NSDocumentsFolderUsageDescription` 같은 키가 필요하다. PPPC 프로필로 미리 허용할 수 있다. 실패 사례로, 탐색 스캔이 프롬프트를 기다리며 약 180초 멈췄고(GAIA), ad-hoc 서명 빌드는 cdhash가 바뀔 때마다 다시 프롬프트를 띄웠다(Cassy). | 동의는 앱별로 받으며 코드 서명에 묶인다. headless daemon은 프롬프트를 띄울 수 없다. | https://developer.apple.com/tutorials/data/documentation/bundleresources/information-property-list/nsdocumentsfolderusagedescription.json ; https://developer.apple.com/tutorials/data/documentation/devicemanagement/privacypreferencespolicycontrol/services-data.dictionary.json ; https://github.com/amd/gaia/issues/5578 ; https://github.com/Richards-LLC/cassy/issues/1299 |
| Windows cloud placeholders / Controlled Folder Access | OS 프라이버시·성능 | Cloud Files placeholder(OneDrive)에는 `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` / `RECALL_ON_OPEN` 속성이 있어, 그 안으로 탐색하면 다운로드가 일어날 수 있다. Controlled Folder Access는 쓰기만 차단하며 기본적으로 꺼져 있다. | 스캐너는 recall 속성이 있는 항목을 건너뛰어야 한다. | https://learn.microsoft.com/en-us/windows/win32/cfapi/build-a-cloud-file-sync-engine ; https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants ; https://learn.microsoft.com/en-us/defender-endpoint/controlled-folders |
| Linux `updatedb` / systemd `ProtectHome` | OS 인덱싱·샌드박스 | `updatedb.conf`에서 `PRUNENAMES`(예: `.git`), `PRUNEPATHS`, `PRUNEFS`를 설정한다. `ProtectHome=`은 서비스에서 `/home`을 숨길 수 있다. | 샌드박스 설정 때문에 daemon이 home을 보지 못할 수 있다. | https://plocate.sesse.net/updatedb.conf.5.html ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/systemd.exec.xml#L1535-L1560 |
| Coder (vscode-coder, agent devcontainer discovery) | 원격 workspace | `vscode://coder.coder-remote/open?...&token=` URI로 workspace를 연다. agent는 자기 디렉터리와 바로 아래 하위 디렉터리에서 프로젝트를 찾으며 `.gitignore`를 따른다. | token이 URI에 실린다. | https://raw.githubusercontent.com/coder/vscode-coder/main/README.md ; https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/agent/agentcontainers/api.go#L441-L540 ; https://raw.githubusercontent.com/coder/terraform-provider-coder/main/docs/resources/agent.md |
| GitHub Codespaces | 원격 workspace | 서비스가 보관하는 목록에서 기존 codespace를 고른다. 파일 시스템 탐색은 없다. | GitHub 계정을 쓴다. | https://raw.githubusercontent.com/github/docs/main/content/codespaces/developing-in-a-codespace/opening-an-existing-codespace.md |
| DevPod | 원격 workspace | git URL, 로컬 경로 또는 이미지로 workspace를 만든다. 소스는 사용자가 지정한다. | provider 자격 증명을 쓴다. | https://raw.githubusercontent.com/loft-sh/devpod/main/docs/pages/developing-in-workspaces/create-a-workspace.mdx |
| systemd user linger (logind) | Linux headless 지속 실행 | `loginctl enable-linger [USER]`를 실행하면 user manager가 부팅 시 시작되고 로그아웃 후에도 유지된다. polkit action `org.freedesktop.login1.set-self-linger`가 `allow_any=yes`이므로 본인 linger는 sudo 없이 켤 수 있다. `UserStopDelaySec` 기본값은 10초다. linger가 꺼져 있으면 로그아웃 시 `/run/user/<uid>`가 삭제된다. WSL에서는 `wsl.conf`에서 systemd를 켜야 한다. | 본인은 polkit을 통해 스스로 켤 수 있고, 다른 사용자에게 켜려면 관리자 권한이 필요하다. | https://www.freedesktop.org/software/systemd/man/latest/loginctl.html ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/src/login/org.freedesktop.login1.policy ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/pam_systemd.xml ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/logind.conf.xml ; https://wiki.archlinux.org/index.php?title=Systemd/User&action=raw ; https://raw.githubusercontent.com/MicrosoftDocs/WSL/main/WSL/systemd.md |
| cloudflared service install | 커넥터 서비스 | `cloudflared service install <token>`은 init 시스템(systemd 또는 SysV)을 감지하고 token을 `/etc/cloudflared` 아래 파일에 저장한다. Windows에서는 실제 서비스로 설치되며, config는 `%PROGRAMDATA%`에 두고 Administrators/SYSTEM에만 권한을 주는 SDDL로 보호한다. tunnel token은 재사용할 수 있고 만료되지 않으며, token을 갱신해도 실행 중인 커넥터는 끊기지 않는다. | 설치에 root/관리자 권한이 필요하다. token은 장기 비밀이다. | https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/linux_service.go ; https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/windows_service.go ; https://raw.githubusercontent.com/cloudflare/cloudflare-docs/production/src/content/docs/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/remote-tunnel-permissions.mdx ; https://raw.githubusercontent.com/cloudflare/cloudflare-docs/production/src/content/partials/cloudflare-one/tunnel/locally-managed/as-a-service/windows.mdx |
| code-server | Linux 서비스 | `User=%i`가 들어간 템플릿 system unit `code-server@.service`를 제공하며, `sudo systemctl enable --now code-server@$USER`로 켠다. | 켜는 데 root가 필요하고, 실행은 해당 사용자 권한으로 한다. | https://github.com/coder/code-server/blob/f07405cba33179e6d2058af07ca2be290f19b262/docs/install.md |
| Tailscale | daemon·등록 | `tailscaled`는 root로 실행된다. Linux에서는 system unit, macOS에서는 LaunchDaemon(`install_darwin.go`가 설치)이다. auth key는 일회용 또는 재사용 가능하며, 1–90일 후 만료(기본 90일)되고 사전 승인할 수 있다. key를 폐기해도 이미 가입한 노드는 해제되지 않는다. node key는 180일 후 만료된다. Windows에서는 `--unattended`를 쓰면 로그인한 사용자 없이도 계속 실행되며, MSI 설치본이 있다. userspace networking을 선택할 수 있다. | auth key가 등록용 비밀이다. key 만료는 노드별로 끌 수 있다. | https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/cmd/tailscaled/tailscaled.service ; https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/scripts/installer.sh ; https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/cmd/tailscaled/install_darwin.go ; https://tailscale.com/docs/features/access-control/auth-keys.md ; https://tailscale.com/docs/concepts/userspace-networking.md ; https://tailscale.com/docs/concepts/macos-variants.md ; https://tailscale.com/docs/how-to/run-unattended.md ; https://tailscale.com/docs/install/windows/msi.md |
| launchd domains | macOS 서비스 | `gui/<uid>` 도메인은 사용자가 콘솔에 로그인해 있을 때만 존재하고, `user/<uid>`는 SSH/백그라운드 세션에서도 존재한다. `LimitLoadToSessionType` 기본값은 Aqua이며, plist에 `[Aqua, Background]`를 지정할 수 있다. SSH에서 `gui/`로 bootstrap하면 오류 125나 5로 실패한다. | 사용자별 agent에는 root가 필요 없다. | https://keith.github.io/xcode-man-pages/launchctl.1.html ; https://keith.github.io/xcode-man-pages/launchd.plist.5.html ; https://github.com/radixhound/pilot-manager/issues/8 ; https://github.com/tpsdev-ai/flair/issues/2040 ; https://github.com/NousResearch/hermes-agent/pull/24275 |
| Homebrew `brew services` | macOS 서비스 | GUI 세션 없이 SSH로 실행하면 `gui/<uid>` 대신 `user/<uid>`를 대상으로 삼는다. | 사용자 수준이다. | https://github.com/Homebrew/brew/blob/9811f64e2cccd62fd8a981c1fed5248b9705fea4/Library/Homebrew/services/system.rb ; https://github.com/Homebrew/brew/blob/9811f64e2cccd62fd8a981c1fed5248b9705fea4/Library/Homebrew/services/cli.rb ; https://github.com/Homebrew/brew/commit/77e026b88d35fe08ef9ae3980fb8e821c3637805 |
| SMAppService | macOS 서비스 등록 | `SMAppService.agent`는 `register()`를 호출하면 바로 활성화된다. daemon은 System Settings에서 관리자 승인이 필요하며, 승인 전 상태는 `requiresApproval`이다. MDM으로 login item을 관리할 수 있다. | Login Items를 통한 사용자 또는 관리자 승인이 필요하다. | https://developer.apple.com/tutorials/data/documentation/servicemanagement/smappservice/register().json ; https://developer.apple.com/tutorials/data/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos.json ; https://developer.apple.com/tutorials/data/documentation/devicemanagement/servicemanagementmanagedloginitems.json |
| GitHub Actions self-hosted runner | 에이전트 등록·서비스 | registration token은 1시간 후 만료되며, 대신 JIT config를 쓸 수 있다. Windows 서비스 설치에는 관리자 권한이 필요하고, 서비스는 기본적으로 `NETWORK SERVICE` 계정으로 실행된다. | 수명이 짧은 registration token을 쓴다. | https://docs.github.com/api/article/body?pathname=/en/rest/actions/self-hosted-runners ; https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Listener/Configuration/NativeWindowsServiceHelper.cs |
| Windows Run / RunOnce keys | Windows 자동 시작 | HKCU/HKLM `Run` 항목은 260자로 제한되고, 실행 시점이 보장되지 않으며, 로그온할 때만 실행된다. | HKCU는 관리자 권한이 필요 없다. | https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys?accept=text/markdown |
| Windows Task Scheduler | Windows 자동 시작 | `InteractiveToken` 작업은 사용자에게 대화형 세션이 있을 때만 실행된다. `S4U`는 로그온 없이 실행되지만 네트워크 자격 증명이 없고 암호화된 파일에 접근할 수 없다. 작업은 `schtasks /create`로 만든다. | `S4U`와 `Password` 로그온 유형은 작업이 받는 자격 증명이 다르다. | https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/TaskSchd/taskschedulerschema-logontype-principaltype-element.md ; https://raw.githubusercontent.com/MicrosoftDocs/windowsserverdocs/main/WindowsServerDocs/administration/windows-commands/schtasks-create.md |
| Windows OpenSSH server | 원격 부트스트랩 | 관리자 계정은 `administrators_authorized_keys`를 쓴다. 기본 셸은 `cmd.exe`다. SSH 세션은 session 0에서 실행되므로 GUI나 대화형 작업을 쓸 수 없다. | 관리자용 key 파일이 따로 있다. | https://raw.githubusercontent.com/MicrosoftDocs/windowsserverdocs/main/WindowsServerDocs/administration/OpenSSH/openssh-server-configuration.md ; https://github.com/PowerShell/Win32-OpenSSH/issues/998 |
| Windows script installers (`irm \| iex`, winget) | 설치 경로 | Deno는 `irm https://deno.land/install.ps1 \| iex`로 설치한다. PowerShell 문서는 `Invoke-Expression` 사용에 주의하라고 경고한다. winget은 패키지 기반의 대안 경로다. | `iex`는 원격 코드를 실행한다. winget 패키지는 서명된 manifest를 쓴다. | https://raw.githubusercontent.com/denoland/deno_install/master/README.md ; https://raw.githubusercontent.com/MicrosoftDocs/PowerShell-Docs/main/reference/7.5/Microsoft.PowerShell.Utility/Invoke-Expression.md ; https://raw.githubusercontent.com/MicrosoftDocs/windows-dev-docs/docs/hub/package-manager/winget/install.md |

### 반복 패턴

- **사용자가 루트를 고르고, `~` 전체를 자동 스캔하지 않는다.** Project Manager, Projectile, GitKraken, ghq, tmux-sessionizer, VS Code는 설정했거나 연 루트만 스캔한다. 자유롭게 스캔하는 Fork와 Sourcetree에서 용량 폭증과 위험 사례가 보고된다(https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/package.json ; https://github.com/fork-dev/Tracker/issues/1094 ; https://jira.atlassian.com/browse/SRCTREE-3001).
- **깊이는 기본적으로 얕고 상한이 있다.** 깊이는 VS Code 1, tmux-sessionizer 1, Project Manager 4, Fork 5–6이고, Projectile은 경로마다 지정한다. 상한에 도달하면 알림 없이 멈추는 경우가 있다(https://github.com/microsoft/vscode/issues/341361 ; https://github.com/fork-dev/Tracker/issues/2067).
- **repo 마커를 만나면 그 아래로 내려가지 않는다.** ghq는 `SkipDir`, Project Manager는 `.git/config`/`gitdir:`, updatedb는 `PRUNENAMES`로 멈춘다. walkdir의 `filter_entry`와 ignore crate가 이 동작을 구현한다(https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/local_repository.go ; https://plocate.sesse.net/updatedb.conf.5.html ; https://github.com/BurntSushi/walkdir/blob/6fd031c82ba5a4204b4ce6eae73dacb00dc072ec/src/lib.rs).
- **판정은 git에 맡기고, worktree는 porcelain 출력으로 나열한다.** GitHub Desktop은 `rev-parse` 한 번으로 판정하고, git 문서도 같은 방식을 권장한다. fzf-git.sh와 VS Code `detectWorktrees`는 worktree를 나열하고, Zed는 identity path로 중복을 제거한다(https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/lib/git/rev-parse.ts ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git-worktree.adoc).
- **dubious ownership 오류를 사용자에게 보여 준다.** VS Code의 `UnsafeRepositoriesManager`와 GitHub Desktop의 `safe.directory` 대화상자가 git 규칙을 따른다(https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/src/model.ts ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/config/safe.adoc).
- **최근 항목은 상한을 두고 로컬에 저장하며 주기적으로 정리한다.** VS Code는 500개, JetBrains는 50개까지 보관한다. Zed는 7일 후 GC하고, zoxide는 90일 후 정리하며, Claude는 30일 보관한다(https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts ; https://github.com/JetBrains/intellij-community/blob/d8c6714c7a3c110c56994d6b0f10814f902100e7/platform/platform-impl/src/com/intellij/ide/RecentProjectsManagerBase.kt ; https://github.com/ajeetdsouza/zoxide/wiki/Algorithm).
- **원격 picker는 스캔하지 않고 home에서 탐색을 시작한다.** VS Code SimpleFileDialog, Zed, JetBrains Gateway가 이렇게 동작한다. Codespaces와 DevPod에서는 사용자가 소스를 지정한다(https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/browser/simpleFileDialog.ts ; https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/open_path_prompt/src/open_path_prompt.rs#L71).
- **캐시는 stale-while-revalidate 또는 세션당 한 번 lazy 로드로 운영한다.** sesh(5초), Projectile, Project Manager가 이렇게 한다(https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/lister/caching_lister.go).
- **에이전트 이력에서는 폴더명이 아니라 세션 파일 안의 `cwd`를 읽는다.** Claude, Codex, Happy가 이렇게 하며, Codex는 파일당 10개 레코드와 최대 10000개 파일로 스캔 범위를 제한한다(https://code.claude.com/docs/en/claude-directory.md ; https://github.com/openai/codex/blob/f3a90545aed3188ee1e5c119ec89a742de399cdc/codex-rs/rollout/src/list.rs).
- **OS 프라이버시 제약이 스캐너 동작을 좌우한다.** macOS TCC는 스캔을 멈추게 하거나 프롬프트를 반복해서 띄운다. Windows placeholder는 탐색만으로 다운로드를 일으키고, Linux `ProtectHome`은 home을 숨긴다(https://github.com/amd/gaia/issues/5578 ; https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/systemd.exec.xml#L1535-L1560).
- **사용자 수준 서비스에는 headless 공백이 있다.** Linux는 linger가 필요하다. macOS는 `gui/`와 `user/` 도메인 구분이 문제가 된다. Windows는 Run key, InteractiveToken, session 0 때문에 로그온에 의존한다(https://github.com/microsoft/vscode/issues/173558 ; https://keith.github.io/xcode-man-pages/launchctl.1.html ; https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/TaskSchd/taskschedulerschema-logontype-principaltype-element.md).
- **상시 실행 에이전트는 root/system 서비스로 설치한다.** Tailscale, cloudflared, code-server, Actions runner, SMAppService daemon이 여기에 해당한다(https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/cmd/tailscaled/tailscaled.service ; https://github.com/coder/code-server/blob/f07405cba33179e6d2058af07ca2be290f19b262/docs/install.md).
- **등록에는 만료되거나 일회용인 token을 쓴다.** Actions runner token은 1시간, Tailscale auth key는 1–90일이다. Tunnels는 device flow를, JetBrains는 `jt=` link를 쓴다. 예외로 cloudflared token은 만료되지 않는다(https://docs.github.com/api/article/body?pathname=/en/rest/actions/self-hosted-runners ; https://tailscale.com/docs/features/access-control/auth-keys.md ; https://raw.githubusercontent.com/cloudflare/cloudflare-docs/production/src/content/docs/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/remote-tunnel-permissions.mdx).

### 상충되는 주장

- **Fork 기본 스캔 깊이는 6인가 5인가.** 메인테이너는 6이라고 했고, 같은 스레드의 이후 사용자는 5라고 보고했다. 아직 결론이 나지 않았으며 버전 간 변경일 가능성이 있다(https://github.com/fork-dev/Tracker/issues/2067 ; https://github.com/fork-dev/Tracker/issues/640 ; https://git-fork.com/releasenotes).
- **VS Code `git.repositoryScanIgnoredFolders` 기본값.** `package.json`은 `["node_modules"]`로 선언하지만, 코드의 fallback은 `[]`이다. 사용자가 실제로 받는 값은 `package.json` 기본값이고, `[]`는 설정이 아예 없을 때만 적용된다(https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/package.json#L3305-L3324 ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/src/model.ts).
- **Claude 프로젝트 폴더명 인코딩.** 문서는 영숫자가 아닌 문자를 모두 `-`로 바꾼다고 설명하지만, Happy의 정규식은 `-`를 그대로 둔다. 어느 쪽이든 원래 경로를 복원할 수 없는 인코딩이므로 실제 경로는 JSONL 안의 `cwd`에서 읽어야 한다(https://code.claude.com/docs/en/claude-directory.md ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-ios/Sources/HappyCore/NewSession/SessionPlaces.swift).
- **ghq 기본 루트.** 코드 주석에는 `~/.ghq`라고 되어 있지만, README와 실제 코드는 `~/ghq`를 쓴다. 코드를 기준으로 `~/ghq`가 맞다(https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/README.adoc ; https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/local_repository.go).
- **linger를 켜는 데 sudo가 필요한가.** VS Code tunnel의 팁은 `sudo loginctl enable-linger`를 안내한다. 반면 polkit 정책은 `set-self-linger allow_any=yes`로 본인 linger를 스스로 켤 수 있게 한다. 상황에 따라 둘 다 맞다. 기본 정책에서 본인에게 켤 때는 sudo가 필요 없고, 다른 사용자에게 켜거나 배포판이 polkit을 강화한 경우에는 sudo가 필요하다(https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_linux.rs ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/src/login/org.freedesktop.login1.policy).

## 5. Ferryx 코드베이스 현재 상태

### 5.1 등록(enrollment)

| 항목 | 현재 상태 | 근거(file:line) |
|---|---|---|
| Account router | 14개 account route를 등록합니다. billing route는 billing이 켜져 있을 때만 merge됩니다. `DefaultBodyLimit` 기본값은 4096이고, relay와 같은 Axum 서버에 마운트됩니다. | service.rs:1697-1737, store.rs:17, relay_server.rs:3109 |
| 이메일 매직링크 로그인 `login/request` | `{origin}/login?code=...`를 메일로 보냅니다. TTL은 600s이고, 이메일별 sliding window로 시간당 5회까지 rate limit이 걸립니다. | service.rs:1701, 750, 763, 204-214; store.rs:13, 16 |
| `login/poll`, `login/consume` | poll은 30일짜리 세션을 발급합니다. consume은 단일 사용으로 처리됩니다. 둘 다 rate limit이 없습니다. | service.rs:816, 884; store.rs:14 |
| `device/request` | 인증 없이 호출됩니다. `device_code`(64 hex), `user_code`(XXXX-XXXX), `email_token`을 만들고 승인 링크를 메일로 보냅니다. TTL 900s, `interval: 2`, `verification_uri={origin}/device`입니다. | service.rs:571, 588, 590-604, 611-626 |
| `device/poll` | `authorization_pending`을 돌려주다가, 승인되면 평문 `enrollmentCode`를 한 번만 전달합니다. | service.rs:637 |
| `device/approve` (GET) | email token 소유만으로 승인됩니다. 사용자가 없으면 만들고, TTL을 600으로 하드코딩한 enrollment code를 발급합니다. 이 code는 `DeviceAuthRecord`에 평문으로 저장됩니다. | service.rs:676, 697-713, 727-734, 737-739 |
| `enrollment-codes` 발급 | bearer 세션이 필요합니다. 32바이트 hex code를 hash해서 저장하고 TTL은 600s입니다. rate limit도, 미사용 code 개수 상한도 없습니다. | service.rs:1709-1712, 960, 449; store.rs:15, 33 |
| `GET /machines` | 소유자의 머신 목록을 돌려줍니다. 마지막으로 보인 지 300s 이내면 online으로 봅니다. 한 번 응답하고 끝나며 push는 없습니다. | service.rs:1713, 981, 48, 158 |
| `enroll/challenge` | 인증 없이 호출됩니다. nonce를 메모리에 120s 동안 보관하고, 같은 `machine_id`로 다시 요청하면 덮어씁니다. | service.rs:1714-1717, 1206, 215 |
| `machines/enroll` | challenge를 소비하고 시간 오차를 120s까지 허용합니다. Ed25519 서명을 검증한 뒤 code를 확인합니다. 실패 시 `ACCOUNT_MACHINE_CLAIMED`/`REMOTE_SUSPENDED`/`PLAN_LIMIT_REACHED`를 돌려줍니다. 성공했을 때만 code를 삭제하고 epoch를 올립니다. | service.rs:1718, 1221, 236, 1250, 1268-1287, 1300, 1321, 1339, 1353; remote/auth.rs:162 |
| grants / peer-record | 머신 grant의 TTL은 600s입니다. peer-record는 머신 서명으로 인증하며 시간 오차를 60s까지 허용합니다. | service.rs:1719-1722, 1413, 31, 1116 |
| 저장소 스키마 | `enrollment_codes`는 hash를 PK로 씁니다. `DeviceAuthRecord`에는 enrollment code가 평문으로 들어갑니다. `machines`에는 `machine_id UNIQUE`가 걸려 있습니다. | store.rs:71-76, 110-118, 78-93; schema.rs:122-128, 159-167, 130-144 |
| Liveness | relay probe가 control channel 존재 여부를 확인하고, `touch_enrolled_machine`이 last_seen을 갱신합니다. | relay_server.rs:630-654; service.rs:512 |
| Plan limits | Free 1대, Pro 10대에 팩당 +5, Team은 좌석 수 기준입니다. 판정식은 `may_enroll_new`입니다. | billing/entitlement.rs:5-9, 69, 157 |
| 클라이언트 enroll / device flow | challenge, 서명, enroll 순서로 진행하고 `account-enrollment.json`을 기록합니다. device flow 쪽은 얇은 HTTP 래퍼입니다. | enroll_client.rs:114-215, 17, 79, 281, 309 |
| CLI | `account enroll --code`, `account login --email`, `account plan`이 있습니다. login은 고정 sleep으로 poll합니다. | src-tauri/src/cli.rs:2248-2252, 2256-2346, 2355-2430 |
| install.sh / install.ps1 | 설정은 env var로만 받고 인자는 없습니다. 끝에 "next steps"를 출력하기만 합니다. | site/public/install.sh:5-15, 262-268; site/public/install.ps1:8-12, 399-403 |
| UI 코드 발급 / EnrollmentStep | 데스크톱이 code를 발급해 `ferryx-cli account enroll --code ...`를 보여 줍니다. "Single use" 배지와 "Refresh" 안내가 붙어 있습니다. | ui/src/remote/accountSession.ts:1050-1084; EnrollmentStep.tsx:7-9, 49-62, 111, 152 |
| UI 머신 목록 / This machine | 한 번 fetch한 뒤에는 수동 Refresh로만 갱신됩니다. ThisMachineSection은 정적 화면입니다. | RemoteSection.tsx:101-133, 135-152, 290-296; ThisMachineSection.tsx:4-9 |
| 갭: `install.sh --key` 없음 | 스크립트가 인자를 받지 않고 안내 문구만 출력합니다. | install.sh:14-15, 211, 262-268; install.ps1:189-194, 296, 399-403 |
| 갭: install-key 타입 없음 | 10분짜리 code는 프로비저닝 스크립트에 쓰기에 너무 짧고, 엄격한 단일 사용도 아닙니다. | store.rs:71; schema.rs:122; service.rs:960, 1266-1353 |
| 갭: CLI `--key`와 이메일 없는 경로 없음 | `login`은 `--email`을 반드시 요구합니다. | cli.rs:2248, 2256, 2374-2381 |
| 갭: device flow가 이메일 전용이고 머신 정보가 없음 | 소유자 세션으로 승인하는 경로가 없고, 요청에 machine_id나 공개키가 실리지 않습니다. | service.rs:571, 676, 536-540; enroll_client.rs:281-307 |
| 갭: device 엔드포인트 rate limit 없음 | 이메일 폭탄과 `device_auths` 행 무한 생성이 가능합니다. | service.rs:763, 571 |
| 갭: `/device` 페이지 없음 | `verification_uri`가 존재하지 않는 경로를 가리키고, CLI도 이 값을 출력하지 않습니다. | service.rs:588; cli.rs:2387-2392 |
| 갭: enrollment code 평문 저장 | `device_auths.enrollment_code`에 평문으로 남습니다. | store.rs:116; service.rs:737-739 |
| 갭: 데스크톱 승인 UI와 실시간 목록 없음 | AddMachineModal에는 enrollment/ssh/import만 있습니다. SSE/WS가 없습니다. | ThisMachineSection.tsx:4; service.rs:981; RemoteSection.tsx:101-133, 290-296; EnrollmentStep.tsx:152; relay_server.rs:643-653 |
| 갭: 고정 sleep poll | `slow_down`과 `expired_token`을 별도 상태로 처리하지 않습니다. | cli.rs:2398-2415 |
| 갭: plan limit을 늦게 알림 | enroll 단계에서야 `PLAN_LIMIT_REACHED`가 나옵니다. | service.rs:1335-1347 |
| 갭: code가 엄격한 단일 사용이 아님 | 거절된 뒤에도 code가 남지만 UI는 "Single use"로 표시합니다. | service.rs:1328-1329, 1345-1346; EnrollmentStep.tsx:111 |
| 갭: `run_blocking` 위반과 비상수시간 비교 | `state.mutate`를 async 스레드에서 직접 호출합니다. hash를 `==`로 비교합니다. | service.rs:1268, 688 |

### 5.2 SSH 설치

| 항목 | 현재 상태 | 근거(file:line) |
|---|---|---|
| Cargo 바이너리 | `ferryx`, `ferryx-cli`, `ferryx-account`, `ferryx-relay`가 정의되어 있습니다. `ferryx-remote-helper`용 `[[bin]]`은 없습니다. | /Volumes/T9-Mac/project/ferryx/src-tauri/Cargo.toml:17-31 |
| Helper 진입점 / 빌드 | standalone `main()`이 있고, 빌드는 `scripts/build-remote-helpers.mjs` 게이트를 거칩니다. | src-tauri/src/ferryx_scope/ssh/standalone.rs:1-8; build.rs:22-34 |
| Helper 버전 / 서브커맨드 | `HELPER_VERSION = "2026.1017.1"`입니다. 서브커맨드는 `start`/`daemon`/`bridge --stdio`뿐입니다. | process.rs:12, 909-931, 42 |
| Helper `start` | Linux에서는 `systemd-run --user --collect` transient unit으로, 안 되면 `setsid`로 띄웁니다. 재부팅하면 살아남지 못합니다. | process.rs:387-470, 580-604, 628-654, 458-466 |
| Helper `daemon` / bridge | `<root>/helper.sock`을 열고 ready 이벤트를 냅니다. bridge는 180s 동안 idle이면 종료됩니다. | process.rs:370-385, 683 |
| 설치 위치 | `~/.ferryx/versions/<ver>/bin/ferryx-remote-helper`, root는 `~/.ferryx/r/<ver>/<sha>`입니다. | src-tauri/src/ssh/helper_setup.rs:85-110, 879-913 |
| 업로드 | base64 heredoc을 `sh -s`로 보내 임시 파일에 쓰고 원자적 `mv`로 옮깁니다. `FERRYX_INSTALL_OK` sentinel로 확인하며 timeout은 60s입니다. | helper_setup.rs:222-293, 785-816, 818-877 |
| `ensure_started` / readiness probe | `exec "$exe" start`를 15s 안에 실행하고, exit 127/126을 구분합니다. probe는 Installed/Missing/Unknown을 돌려줍니다. | helper_setup.rs:406-450, 295-321, 323-404, 522-627 |
| Target triple 해석 | Windows는 x64로 고정합니다. POSIX는 `uname -s; uname -m` 결과로 정합니다. | src-tauri/src/ipc/ssh.rs:722-766; helper_assets.rs:82-122 |
| `ensure_qualified_ssh_helper` | 없으면 번들 바이너리를 sha256과 길이로 검증한 뒤 설치하고 시작합니다. remote PTY spawn이나 DAG 등에서 호출됩니다. | ipc/ssh.rs:768-794; daemon/session_service.rs:1850; ipc/dag.rs:878; ipc/ssh.rs:886 |
| 번들 manifest | `helpers/manifest.json`(schema 1)이고, `helperVersion == HELPER_VERSION`이어야 합니다. | ipc/ssh.rs:609-651, 653-685; helper_assets.rs:124-321; tauri.conf.json:49 |
| 수동 설치 명령 | `cmd_ssh_install_project_helper`와 `cmd_ssh_provision_helper`는 사용자가 고른 로컬 바이너리를 설치합니다. | ipc/ssh.rs:563-582, 971-984; helper_setup.rs:915-925 |
| 업데이트 판정 | calver 비교로 Install/NoOp/Upgrade를 정합니다. | ipc/ssh.rs:986-1047; helper_setup.rs:736-772 |
| SSH transport | 시스템 `ssh`를 씁니다(russh 아님). `BatchMode=yes`, `StrictHostKeyChecking=yes`, `ControlMaster=no` 옵션을 붙입니다. | src-tauri/src/ssh/exec.rs:6-79; direct.rs:105-162 |
| 비밀번호 처리 | 메모리에만 두고, loopback askpass 브로커가 UUID token으로 넘깁니다. argv, env, 파일 어디에도 남지 않으며 daemon으로 전달됩니다. | password.rs:1-2, 43-63, 101-145, 147-195, 216; ipc/ssh.rs:500-562, 483-499 |
| 키 처리 | agent, identity_file, `~/.ssh/config`를 그대로 씁니다. Ferryx가 직접 관리하는 키는 없습니다. | direct.rs:119-147 |
| `ferryx-cli` 기능 | `account`, `--daemon`, `--daemon-supervisor`가 있습니다. 서비스 설치 서브커맨드는 없습니다. | src-tauri/src/bin/cli.rs:39-50, 96-115; cli.rs:2940 |
| 로컬 enroll IPC | 이 머신만 enroll합니다. | src-tauri/src/ipc/account.rs:33-60 |
| UI SSH 탭 | password 설정, 연결 테스트, `updateSshHost`, `onSuccess`까지만 합니다. 설치나 enroll 단계는 없습니다. | ui/src/components/settings/AddMachineModal.tsx:395-460; useSshActions.ts:55-120 |
| Loopback SSH QA | 실제 OpenSSH와 PTY를 쓰는 회귀 테스트가 있습니다. | daemon/remote_ssh_tests.rs:1-2, 22-23, 181; daemon/server.rs:7876 |
| 갭: CLI per-triple 아티팩트 없음 | 번들에는 helper만 들어 있습니다. | helper_assets.rs:31-39; build.rs:22 |
| 갭: CLI 원격 설치 경로 없음 | `qualified_location`이 helper 파일명을 하드코딩하고, defer 판단도 helper pid 기준입니다. | helper_setup.rs:879, 785, 818 |
| 갭: CLI `service install` 없음 | supervisor는 GUI에서만 호출됩니다. | supervisor.rs:172; cli.rs:2256 |
| 갭: Linux linger 미처리 | user bus가 없으면 거부하고, linger 확인도 요청도 하지 않습니다. | supervisor_linux.rs:474 |
| 갭: code가 argv에 노출 | `--code <code>`가 `ps`와 history에 남습니다. `--code-stdin`이 필요합니다. | cli.rs:2272-2276 |
| 갭: daemon이 enroll 기록을 다시 읽는지 미확인 | `load_enrollment_record`를 lazy하게 읽는다는 것까지만 확인했습니다. | daemon/server.rs:6223; remote/relay_client.rs:749 |
| 갭: 단계를 묶는 명령 없음 | 기존 명령은 helper 설치까지만 합니다. | ipc/ssh.rs:768, 971 |
| 갭: UI가 "saved"에서 끝남 | code 발급기가 있지만 enrollment 탭에만 연결되어 있습니다. | AddMachineModal.tsx:395-460, 699-702; RemoteSection.tsx:158 |
| 갭: enroll 후 검증 없음 | `/machines`에서 실제로 등록됐는지 확인하지 않습니다. | service.rs:980 |

### 5.3 디스커버리

| 항목 | 현재 상태 | 근거(file:line) |
|---|---|---|
| 머신 카탈로그 경로 | `<identity_dir>/machine-workspaces.v1.json`이고, 만드는 곳은 `DaemonWorkspaceService::new` 하나뿐입니다. | /Volumes/T9-Mac/project/ferryx/src-tauri/src/daemon/server.rs:3206-3211, 3220 |
| 카탈로그 스키마 | 명시적으로 등록한 root만 담습니다(`CatalogRow`, `Ownership`, `worktree_observations`). | src-tauri/src/remote/workspace_catalog.rs:11-49 |
| 로드 / 저장 | 손상 시 실패로 닫고(fail-closed) 격리합니다. 저장은 `write_private_json`에 디렉터리 fsync까지 합니다. | workspace_catalog.rs:99-153 |
| 부팅 시 재확인 | 기존 행만 다시 stat하고, 새로 스캔하지는 않습니다. | src-tauri/src/daemon/workspace_service.rs:53-99 |
| 데스크톱 / 머신 측 등록 | `register`(canonical git root), `sync_desktop_local_projects`, `register_machine`(`project-<uuid>`)가 있습니다. | workspace_service.rs:232-304, 306-361, 369-428 |
| HTTP route | `fs/directories`, `workspace/projects`, `workspace/worktrees`가 있고 discovery route는 없습니다. | src-tauri/src/remote/server.rs:7364-7367, 7462-7514 |
| Capability 게이트 | Machine+Control일 때만 `directoryBrowseV1`/`machineWorkspaceV1`/`managedWorktreesV1`을 줍니다. | server.rs:3859 |
| Project inventory | 카탈로그 행만 나열합니다. 최대 4 스레드로 probe하고 결과는 1 MiB로 제한합니다. | src-tauri/src/remote/workspace_api.rs:775-919, 927-936 |
| root별 git probe | `rev-parse --show-toplevel`/`--git-common-dir`/HEAD/origin을 확인하고 credential은 제거합니다. | workspace_api.rs:653-734 |
| HTTP 등록 | `~`를 해석하고 canonical 처리한 뒤 `show-toplevel`로 정규화합니다. 장치별로 멱등입니다. | workspace_api.rs:968-1062, 327-350 |
| 디렉터리 브라우즈 | 임의의 절대 경로를 읽기 전용으로 엽니다(home 제한 없음). 1000개, 256 KiB, 5s 제한과 token bucket이 있고 git은 인식하지 못합니다. | src-tauri/src/remote/filesystem.rs:117-148, 177-307, 437-560 |
| Worktree 목록 | 등록된 `workspaceId`에 대해서만 `git worktree list --porcelain -z`를 실행합니다. | src-tauri/src/remote/workspace_api/worktrees.rs:95-178, 284-300 |
| Wire DTO | `Project`, `Projects`, `Worktree`, `Worktrees`가 있습니다. | src-tauri/src/remote/machine_protocol.rs:211-283 |
| Paired 클라이언트 / projection | operation을 relay 경로로 매핑하고, id를 `daemon:<sha>`로 다시 씁니다. `paired_projects.json` 캐시가 있습니다. | src-tauri/src/paired_host/client.rs:320-360, 1386-1405; paired_host/projects.rs:20-50, 81-101 |
| Paired host inventory | HostView만 담고 프로젝트 데이터는 없습니다. | paired_host/inventory.rs:12, 56-75, 337-363 |
| SSH 대응 경로 | 별도의 브라우즈와 `remote_projects.json`이 있지만 스캔은 없습니다. | src-tauri/src/ssh/browse.rs:44; src-tauri/src/ssh/projects.rs:17-48 |
| UI adapter / add flow | 수동 폴더 피커, `registerProject`, `onRegistered` 순서로 진행합니다. | ui/src/lib/pairedDaemonProject.ts:16-29, 152-175; ui/src/components/PairedDaemonProjectForm.tsx:20-111 |
| UI 진입점 / worktree 목록 | 머신 행의 "Add Project"에서 시작합니다. worktree는 사이드바에 이미 있는 프로젝트에 대해서만 나옵니다. | RemoteSection.tsx:580; MachineRow.tsx:205-208; App.tsx:2946-2955, 1022-1028; ProjectDialogs.tsx:114, 338-341; ui/src/state/pairedProjectWorktrees.ts:13-48 |
| "Discovered" 목록 | 레포가 아니라 Tailscale peer를 보여 줍니다. | ui/src/components/settings/remote/DiscoveredList.tsx:22-25 |
| 갭: 레포 디스커버리 없음 | `WalkDir`나 `discover_repos` 같은 코드가 0건입니다. | filesystem.rs:119; workspace_api.rs:351, 373 (재사용 대상) |
| 갭: discovery route / capability 없음 | — | server.rs:7364, 3859 |
| 갭: relay allowlist 미확인 | 새 경로에 대한 query 검증이 필요할 수 있지만 확인하지 못했습니다. | relay_server.rs:4357-4376, 6940 |
| 갭: 클라이언트 operation 없음 | `DiscoverRepos` variant가 필요하고, 발견된 행에는 `daemon:` 재작성을 하면 안 됩니다. | client.rs:320-360, 1386 |
| 갭: 미등록 checkout의 worktree 조회 불가 | `worktrees::read`가 카탈로그 id를 요구합니다. | worktrees.rs:284-300 |
| 갭: 머신 전체 카탈로그 미표시 | `adapter.projects()`는 readiness 확인에만 쓰입니다. | pairedProjectWorktrees.ts:13-48 |
| 갭: add flow가 피커뿐임 | — | PairedDaemonProjectForm.tsx:93-111 |
| 갭: UI adapter 메서드 없음 | — | pairedDaemonProject.ts:17-29, 74-79, 152-175 |
| 갭: linked worktree가 별도 프로젝트가 됨 | `show-toplevel`이 worktree 경로를 돌려주기 때문입니다. | workspace_api.rs:1050-1054 |
| 갭: `paired_projects.json` 비원자적 쓰기 | `std::fs::write`를 쓰고 파싱 실패 시 `unwrap_or_default`로 넘어갑니다. | paired_host/projects.rs:89-95 |

### 5.4 서비스 등록

| 항목 | 현재 상태 | 근거(file:line) |
|---|---|---|
| Supervisor facade | macOS는 launchd LaunchAgent, Windows는 scheduled task와 watchdog, Linux는 systemd user unit을 씁니다. | /Volumes/T9-Mac/project/ferryx/src-tauri/src/daemon/supervisor.rs:1-24 |
| `supported()` 정책 | dev 빌드이거나 `FERRYX_DATA_DIR`/`RUNTIME_DIR`/`SESSION_DIR`가 설정되어 있으면 비활성화됩니다. | supervisor.rs:56-83 |
| `ensure_if_absent` / admission | 잠금, owner 분류, legacy 마이그레이션, owner가 Absent일 때만 시작, login 등록 순서로 진행합니다. | supervisor.rs:198-209; daemon/owner_admission.rs:914-955 |
| 공개 lifecycle / opt-out | `ensure`, `uninstall`, `remove_autostart`, `installed`와 `daemon-supervisor.optout` 마커가 있습니다. | supervisor.rs:124-185, 306-311, 86-104 |
| 자동 설치 경로 | Tauri `setup`에서만 실행됩니다. | src-tauri/src/lib.rs:1411 |
| Settings 토글 IPC | GUI 전용입니다. | src-tauri/src/ipc/agent_integration.rs:77-113 |
| Client fallback / detached spawn | supervisor를 쓸 수 없으면 `setsid`로 detached spawn합니다. 이 경우 재시작도 부팅 시 자동 시작도 없습니다. | daemon/client.rs:2676-2694, 2762-2775; src-tauri/src/util/mod.rs:118-138 |
| macOS plist | `com.ferryx.daemon`, `RunAtLoad`, `KeepAlive{SuccessfulExit=false}`로 설정되고 로그는 `~/Library/Logs/Ferryx`에 남습니다. | daemon/launchd.rs:42, 59-75, 128-178 |
| macOS job 전환 / 제거 | 항상 `gui/<uid>` 도메인에 bootstrap/kickstart합니다. 제거는 `unload -w`입니다. | launchd.rs:462-497, 302-303, 686-716 |
| macOS helper 번들 staging | `Helpers/Ferryx Daemon.app`이 없으면 `HelperMissing`이 됩니다. | supervisor.rs:343-348; daemon/runtime_stage.rs:121-139 |
| Linux unit | `Type=notify`, `Restart=on-failure`, `KillMode=process`, `WantedBy=default.target`입니다. | daemon/supervisor_linux.rs:430-457 |
| Linux 경로 / 이름 | `~/.config/systemd/user/ferryx-daemon-<key>.service`입니다. | supervisor_linux.rs:37, 148-162; supervisor.rs:648-674 |
| Linux 가용성 probe | WSL, 컨테이너, user manager 없음은 unsupported로 처리합니다. | supervisor_linux.rs:474-519 |
| Linux systemctl 래퍼 | `XDG_RUNTIME_DIR`가 없으면 fallback 값을 씁니다. `enable-linger`는 호출하지 않습니다. | supervisor_linux.rs:523-544 |
| Linux start / login 등록 / secret | `start --no-block`과 `enable`을 쓰고 `--now`는 쓰지 않습니다. secret은 0600 env 파일에 둡니다. | supervisor_linux.rs:576-617, 622-632, 672-681, 275-280, 323-379 |
| Linux sd_notify | 바인딩 후 `READY=1`을 보냅니다. user manager가 없으면 그 사실을 로그로 남깁니다. | supervisor_linux.rs:773-834; cli.rs:2949-2950, 3053-3054 |
| Windows task | `Ferryx\Daemon-<key>`, `LogonTrigger`, `InteractiveToken`, `RestartOnFailure`입니다. | daemon/supervisor_windows.rs:186-235 |
| Windows ensure / watchdog | Query, Create, Run 순서입니다. watchdog은 1s backoff에 분당 최대 5회 재시작합니다. | supervisor_windows.rs:389-415, 824-880, 1259; cli.rs:2887-2917 |
| `ferryx-cli` 서브커맨드 | `agents`…`remote`, `--daemon`, `--daemon-supervisor`가 있고 `service`/`up`/`install`은 없습니다. | src-tauri/Cargo.toml:21-23; src-tauri/src/bin/cli.rs:1-115; cli.rs:2866-2880 |
| `account enroll` / `login` | daemon이 실행 중인지 확인하지 않습니다. | cli.rs:2248-2337, 2355-2435; account/enroll_client.rs:114-215, 281-330 |
| Runtime dir | Unix는 `/tmp/rorca-<uid>[-dev]/daemon.sock`, Windows는 `daemon.port`입니다. | daemon/server.rs:540-578, 857-866 |
| 설치 스크립트 | `ferryx-cli --daemon &`과 `account login` 안내만 출력하고 서비스는 등록하지 않습니다. | site/public/install.sh:51-62, 214-268; site/public/install.ps1:345-352 |
| Headless Linux 가이드 | 손으로 작성하는 unit, `loginctl enable-linger`, `enable --now` 절차를 안내합니다. | docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:4-5, 226-293, 295-323, 468-475 |
| 갭: CLI 서비스 명령 없음 | supervisor는 GUI와 daemon client에서만 호출됩니다. | bin/cli.rs:15-114, 110; lib.rs:1411; ipc/agent_integration.rs:99-103; daemon/client.rs:2683 |
| 갭: CLI 전용 설치에 자동 시작 없음 | 스크립트가 수동 시작 명령만 출력합니다. | install.sh:262-268; install.ps1:345-352; HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:236-283 |
| 갭: macOS standalone CLI는 `HelperMissing` | — | runtime_stage.rs:123-139; supervisor.rs:343-348 |
| 갭: macOS `gui/<uid>` 하드코딩 | SSH 전용 사용자는 bootstrap이 실패합니다(error 125). | launchd.rs:302-303, 479-490; supervisor.rs:333-336 |
| 갭: Linux linger 없음 | logout 시 user slice와 함께 daemon이 종료됩니다. | supervisor_linux.rs:496-505 |
| 갭: secret env의 `XDG_RUNTIME_DIR` fallback 없음 | 값이 없으면 토큰 동기화를 조용히 건너뜁니다. | supervisor_linux.rs:383-407, 531-533 |
| 갭: `FERRYX_DATA_DIR` 사용 시 서비스 불가 | — | supervisor.rs:56-69; HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md:651-660; supervisor_linux.rs:178-188 |
| 갭: Windows는 로그온해야 실행됨 | `InteractiveToken` 때문입니다. install.ps1은 AMD64만 허용합니다. | supervisor_windows.rs:203; install.ps1:262-268 |
| 갭: enroll과 daemon이 분리됨 | 재시작 없이 새 enroll 기록을 반영하는지 확인하지 못했습니다. | enroll_client.rs:121-201; relay_client.rs:746-752 |
| 갭: 서비스 상태 표면 없음 | `remote status`는 gateway 상태만 보여 줍니다. | cli.rs:2471-2531 |

## 6. Ferryx 권고안

세 reduce 파일의 Recommendations를 합치고 겹치는 항목을 정리했습니다. 순위는 우선순위 순입니다.

| # | 결정(ADOPT/AVOID/DEFER) | 권고 | 근거 제품 | 출처 URL |
|---|---|---|---|---|
| 1 | ADOPT | **단일 사용 install key 도입.** TTL은 기본 1h 이하로 짧게, hash로 저장하고 한 번만 보여 줍니다. `install.sh --key`/`FERRYX_INSTALL_KEY`/`install.ps1 -Key`로 받습니다. POST로만 소비하고(GET이나 링크 미리보기로는 소비되지 않게), 새 key가 나오면 이전 key는 무효화하며, 두 번째 claim에는 409를 돌려줍니다. 결과가 어떻든 key를 소모시켜 "Single use" 표시와 실제 동작을 맞춥니다. | Headscale, NetBird, Superset, Devin | https://github.com/juanfont/headscale/blob/main/cmd/headscale/cli/preauthkeys.go ; https://docs.netbird.io/how-to/register-machines-using-setup-keys ; https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/plans/20261010-add-host-over-ssh.md ; https://docs.devin.ai/work-with-devin/local-hands.md |
| 2 | ADOPT | **key는 기존 Ed25519 머신 키페어 등록에 한 번만 쓰고 저장하지 않습니다.** 장기 신원은 머신에서 생성한 키가 맡습니다. 승인된 code는 `machine_id`와 공개키에 바인딩합니다. | GitHub Actions runner, Balena, Devin Outposts | https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners ; https://raw.githubusercontent.com/balena-io/docs/a0d2eb7d44495b1bc835db38b2417f52332e71ff/pages/learn/welcome/security.md ; https://docs.devin.ai/cloud/outposts/partners.md |
| 3 | ADOPT | **secret을 argv로 넘기지 않습니다.** `account enroll --code-stdin`, env 또는 파일로 받고, 오류 출력에서도 redact합니다. | cloudflared(`--token-file` 수정), Tailscale(`--auth-key=file:`), argv 노출 | https://github.com/cloudflare/cloudflared/commit/f70adda11cf4c1acf1aa9c1df952baa05269f074 ; https://tailscale.com/kb/1241/tailscale-up ; https://man7.org/linux/man-pages/man5/proc_pid_cmdline.5.html |
| 4 | ADOPT | **SSH "Add machine" 한 번에 등록까지.** `cmd_ssh_install_ferryx_node`가 CLI 업로드, 데스크톱이 발급한 머신 범위 code의 stdin 전달과 enroll, `service install`(linger 확인 포함), `/machines`에서 online 확인까지 이어서 처리합니다. 계정 전체 API key는 쓰지 않습니다. | Superset `hosts add`, Wave(stdin), VS Code Tunnels(service) | https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/packages/cli/src/commands/hosts/add/command.ts ; https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/conncontroller/conncontroller.go ; https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/service_linux.rs |
| 5 | ADOPT | **크로스플랫폼 `ferryx-cli up` / `service install\|status\|uninstall`.** Linux는 user unit을 쓰고 `loginctl enable-linger $USER`를 실행합니다(polkit self 허용, 실패하면 sudo 안내). macOS는 `gui/` 또는 `user/<uid>` 도메인을 고르고 `LimitLoadToSessionType [Aqua, Background]`를 설정하며, CLI에서는 Helpers 번들을 요구하지 않습니다. Windows는 기본으로 logon task를 쓰고, headless 환경에서는 opt-in으로 S4U나 실제 서비스를 씁니다. 실행 중인 daemon은 절대 죽이지 않습니다. | systemd loginctl/polkit, Homebrew `brew services`, launchd, Windows Task Scheduler | https://www.freedesktop.org/software/systemd/man/latest/loginctl.html ; https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/src/login/org.freedesktop.login1.policy ; https://github.com/Homebrew/brew/blob/9811f64e2cccd62fd8a981c1fed5248b9705fea4/Library/Homebrew/services/system.rb ; https://keith.github.io/xcode-man-pages/launchctl.1.html ; https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/TaskSchd/taskschedulerschema-logontype-principaltype-element.md |
| 6 | ADOPT | **device flow 강화(fallback으로 유지).** `device/request`와`device/poll`에 rate limit을 걸고 `slow_down`을 지킵니다. device code는 hash로 저장하고 상수시간으로 비교합니다. 승인은 CSRF 보호를 받는 POST로 바꾸고, 머신 이름, OS, IP를 보여 주며 기본값은 거절로 둡니다. "누군가 이 code를 줬다면 취소하세요" 경고를 넣고 `/device` 페이지를 실제로 만들거나 응답에서 제거합니다. | RFC 8628, IETF cross-device BCP, Microsoft Entra(Storm-2372) | https://www.rfc-editor.org/rfc/rfc8628.txt ; https://www.ietf.org/archive/id/draft-ietf-oauth-cross-device-security-16.txt ; https://www.microsoft.com/en-us/security/blog/2025/02/13/storm-2372-conducts-device-code-phishing-campaign/ |
| 7 | ADOPT | **데스크톱 대기 승인 큐와 실시간 push.** CLI 요청은 `user_code`를 앵커로 데스크톱에서 Approve/Deny합니다. `GET /api/account/v1/events` SSE로 머신 목록과 presence를 갱신하고, 수동 Refresh 안내는 없앱니다. | Tailscale device approval, Syncthing pending devices, Portainer waiting room | https://tailscale.com/kb/1099/device-approval ; https://docs.syncthing.net/rest/cluster-pending-devices-get.html ; https://docs.portainer.io/user/edge/waiting-room.md |
| 8 | ADOPT | **범위가 제한된 레포 디스커버리.** `repoDiscoveryV1` capability 아래 `GET /api/v1/fs/repositories`를 둡니다. home 바로 아래와 사용자가 지정한 root에서 시작하고, depth는 3~4 이하, `.git`을 만나면 하위로 내려가지 않습니다. `ignore::WalkBuilder`에 `follow_links(false)`를 쓰고 `node_modules` 등을 건너뜁니다. 기존 개수, 시간, 바이트 상한을 재사용하고, 상한에 걸리면 `truncated: true`를 돌려줍니다. | Orca `scanNestedRepos`, ignore crate, VS Code git(조용한 상한 문제) | https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/project-groups/nested-repo-discovery.ts ; https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html ; https://github.com/microsoft/vscode/issues/341361 |
| 9 | ADOPT | **git으로 분류하고 worktree를 묶으며, dubious ownership은 그대로 드러냅니다.** `rev-parse --is-bare-repository --git-dir --git-common-dir --show-toplevel`을 한 번 실행하고, common dir 기준으로 linked worktree를 부모 프로젝트에 붙입니다(`workspace_api.rs:1050-1054` 수정). `2>/dev/null`을 없애고 `unsafe_repository` 구조화 오류와 `safe.directory` 처리를 제공합니다. | GitHub Desktop, git worktree/safe.directory, VS Code `UnsafeRepositoriesManager` | https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/lib/git/rev-parse.ts ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git-worktree.adoc ; https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/config/safe.adoc ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/src/model.ts |
| 10 | ADOPT | **설치 스크립트 강화.** checksum이나 서명을 검증하고, 스크립트 전체를 `main()`으로 감싸며, `curl --proto =https`를 쓰고 재실행해도 안전하게 만듭니다. macOS는 sudo 없이 LaunchAgent를 쓰고, sleep 대신 `FERRYX_DAEMON_READY`를 기다립니다. | Buildkite, Tailscale installer, NetBird, GitHub Actions `svc.sh` | https://raw.githubusercontent.com/buildkite/agent/main/install.sh ; https://github.com/tailscale/tailscale/blob/main/scripts/installer.sh ; https://pkgs.netbird.io/install.sh ; https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Misc/layoutbin/darwin.svc.sh.template |
| 11 | ADOPT | **아티팩트 검증, 버전 관리, 호환성.** 아티팩트는 검증한 뒤에 원자적으로 rename합니다(`.unverified` 단계). 버전별 디렉터리를 나란히 두고 멈춘 버전만 정리합니다. sha256 manifest를 per-triple `ferryx-cli` 아티팩트까지 넓힙니다. 프로토콜은 major.minor로 협상하고, 버전이 맞지 않으면 실패 대신 "agent outdated" 상태로 낮춰 동작합니다. | Tailscale distsign, VS Code server cache, Coder apiversion, Teleport skew | https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/clientupdate/distsign/distsign.go ; https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/paths.rs ; https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/apiversion/apiversion.go ; https://goteleport.com/docs/upgrading/overview.md |
| 12 | ADOPT | **한 번의 exec로 끝나는 probe와 타입이 있는 오류.** sentinel 줄로 glibc/musl, 쓰기 가능한 디렉터리, `/etc/machine-id` 중복, `AllowTcpForwarding`을 확인합니다. CLI는 expired, used, plan-limit, suspended를 서로 다른 exit code로 돌려줍니다. plan limit은 key 발급 시점에 미리 확인합니다. | DevPod inject, Netdata exit code, Wave probe | https://github.com/loft-sh/devpod/blob/5a0efcbff6610ab114b421f68a890739a452e66b/pkg/inject/inject.sh ; https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/packaging/installer/kickstart.sh#L1611-L1628 ; https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/connutil.go |
| 13 | ADOPT | **key 폐기와 머신 제거를 분리합니다.** 머신별 키페어를 두고 revision counter로 폐기합니다. Revoke와 Offline은 별개 동작으로 둡니다. key 화면에는 그 key로 등록된 머신을 보여 줍니다. | Orca device-registry, Codex remote connections, Tailscale auth keys | https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/runtime/device-registry.ts ; https://learn.chatgpt.com/docs/remote-connections.md ; https://tailscale.com/kb/1085/auth-keys |
| 14 | ADOPT | **에이전트 기록의 `cwd`로 추천을 채웁니다.** Claude JSONL과 Codex rollout은 앞부분만 제한해서 읽고 파일 개수에도 상한을 둡니다. 경로는 폴더명이 아니라 레코드의 `cwd`를 씁니다. 순서는 zoxide식 frecency로 정합니다. | Claude Code, Codex CLI, zoxide | https://code.claude.com/docs/en/claude-directory.md ; https://github.com/openai/codex/blob/f3a90545aed3188ee1e5c119ec89a742de399cdc/codex-rs/rollout/src/list.rs ; https://github.com/ajeetdsouza/zoxide/wiki/Algorithm |
| 15 | ADOPT | **home에서 시작하는 원격 피커가 OS 프라이버시 제약을 견디게 합니다.** macOS TCC 보호 폴더는 동의 없이 스캔하지 않고, timeout이 나면 건너뜁니다. Windows에서는 recall 속성이 있는 placeholder를 건너뜁니다. | macOS TCC(GAIA 사례), Windows Cloud Files | https://github.com/amd/gaia/issues/5578 ; https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants |
| 16 | ADOPT | **recents를 원자적으로 쓰고 상한과 GC를 둡니다.** `paired_projects.json`은 임시 파일에 쓴 뒤 rename하고, 약 50~500개로 제한하며, 사라진 경로는 정리합니다. | Zed persistence, JetBrains recent projects | https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/workspace/src/persistence.rs#L2723-L2775 ; https://github.com/JetBrains/intellij-community/blob/d8c6714c7a3c110c56994d6b0f10814f902100e7/platform/platform-impl/src/com/intellij/ide/RecentProjectsManagerBase.kt |
| 17 | ADOPT | **위생 작업(비용 낮음).** account handler의 `state.mutate`를 `run_blocking`으로 옮겨 AGENTS.md 규칙을 지킵니다. 오래 오프라인인 머신은 stale로 표시하거나 정리합니다. | GitHub Actions(14일 자동 제거), GitLab Runner(7일 stale) | https://docs.github.com/en/actions/reference/runners/self-hosted-runners ; https://docs.gitlab.com/ci/runners/configure_runners/ |
| 18 | AVOID | **재사용 가능하거나 만료되지 않는 토큰을 기본값으로 두지 않습니다.** poll할 때마다 토큰을 새로 발급하는 방식, 원격의 계정 전체 자격증명, 평문이나 0644 secret 파일도 피합니다. | cloudflared, Rancher GHSA-7r53, Happy(#1830), New Relic | https://developers.cloudflare.com/tunnel/reference/tunnel-tokens/index.md ; https://github.com/rancher/rancher/security/advisories/GHSA-7r53-jvhg-9jq4 ; https://github.com/slopus/happy/issues/1830 ; https://github.com/newrelic/newrelic-cli/blob/4c62d7ab4133f0b082bba2ad10daca6fa7211570/internal/install/command.go |
| 19 | AVOID | **"먼저 온 사람이 이기는" 무인증 claim과, 장치 정보 없이 하는 맹목 승인을 쓰지 않습니다.** | Home Assistant, Jellyfin, Coder `/cli-auth`, Happy | https://github.com/home-assistant/core/pull/169299 ; https://github.com/jellyfin/jellyfin/pull/17369 ; https://raw.githubusercontent.com/coder/coder/main/site/src/pages/CliAuthPage/CliAuthPageView.tsx ; https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-server/sources/app/api/routes/authRoutes.ts |
| 20 | AVOID | **`StrictHostKeyChecking no`와 base64 host key 표시를 쓰지 않습니다.** 현재의 `=yes`를 유지하고, SHA256 TOFU로 확인하며, 불일치하면 자격증명을 보내기 전에 실패시킵니다. | DevPod(반례), Orca host-key store(모범) | https://github.com/loft-sh/devpod-provider-ssh/blob/master/pkg/ssh/ssh.go ; https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/ssh/ssh-host-key-store.ts |
| 21 | AVOID | **home 전체를 무제한 재귀 스캔하지 않고, 발견한 디렉터리의 설정 파일을 source하거나 실행하지 않습니다.** | Fork, Sourcetree, tmux-sessionizer | https://github.com/fork-dev/Tracker/issues/2067 ; https://jira.atlassian.com/browse/SRCTREE-3001 ; https://github.com/ThePrimeagen/tmux-sessionizer/blob/7edf8211e36368c29ffc0d2c6d5d2d350b4d729b/tmux-sessionizer |
| 22 | DEFER | **사용 횟수와 만료가 있는 재사용 fleet key.** 실제 fleet 수요가 생기면, 대기 승인을 필수로 하는 조건에서만 검토합니다. | Mender, Tailscale reusable/pre-approved key | https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/02.Overview/14.Device-authentication/docs.md ; https://tailscale.com/docs/features/access-control/auth-keys.md |
| 23 | DEFER | **무인 자가 업데이트.** holdback, 유지보수 창, canary, rollback이 함께 있어야 합니다. | Tailscale auto-update, Teleport Managed Updates | https://tailscale.com/docs/features/client/update.md ; https://goteleport.com/docs/upgrading/agent-managed-updates.md |
| 24 | DEFER | **Tailnet Lock식 머신 키 서명, SMAppService나 root/system daemon 등록.** notarization과 per-user 서비스가 안정된 뒤에 검토합니다. | Tailscale Tailnet Lock, Apple SMAppService | https://tailscale.com/kb/1226/tailnet-lock ; https://developer.apple.com/tutorials/data/documentation/servicemanagement/smappservice/register().json |
| 25 | DEFER | **다른 IDE recents 읽기와 ssh_config host import.** 각각 VS Code/Cursor `state.vscdb`를 읽는 것과, `Include`를 따라가며 ProxyCommand를 알려 주는 것을 뜻합니다. 유용하지만 다른 앱의 비공개 저장 형식에 묶입니다. | Raycast Cursor Recent Projects, Tabby | https://github.com/raycast/extensions/blob/26ee20de2d66c9d8b18225d9fc4211a2f0689387/extensions/cursor-recent-projects/src/db.ts ; https://github.com/Eugeny/tabby/blob/467ce8ebfa467fb88cf6d154c9a5891e3d3647f0/tabby-electron/src/sshImporters.ts |

## 7. 결정 사항 (이번 구현 범위)

| 영역 | 결정 |
|---|---|
| SSH 경로 | 데스크탑이 단일 사용 설치 키를 발급해 SSH stdin으로 원격 `install.sh`에 넘깁니다. 원격은 CLI를 설치한 뒤 `ferryx-cli up --key-stdin`으로 계정 등록과 데몬 서비스 등록을 합니다. 키는 argv나 셸 히스토리에 남지 않습니다. 호스트 키 검증(`StrictHostKeyChecking=yes`)은 유지합니다. |
| 비SSH 경로 (설치 키) | 앱이 키를 넣은 원라이너를 보여줍니다: `curl -fsSL https://relay.ferryx.dev/install.sh \| sh -s -- --key fxk_…` 와 PowerShell 대응 명령. |
| 비SSH 경로 (승인) | 키 없이 `ferryx-cli up`을 실행하면 CLI가 승인 URL과 사용자 코드를 출력합니다. 로그인한 소유자는 앱의 승인 카드나 웹 페이지에서 코드를 입력해 요청에 바인딩하고, 머신 이름, OS, IP를 확인한 뒤 승인합니다. 기본 동작은 거절입니다. CLI는 폴링하다가 승인되면 등록합니다. |
| 폴백 | 기존 이메일 매직 링크(`account login --email`)를 유지합니다. |
| 키 정책 | 단일 사용입니다. TTL은 기본 1시간이고 10분–24시간 범위에서 정할 수 있습니다. 해시로 저장하고 계정에 바인딩합니다. 재사용하면 409 `INSTALL_KEY_USED`, 만료되면 401 `INSTALL_KEY_EXPIRED`를 돌려줍니다. 다른 계정의 키로는 그 계정에만 등록되므로 계정 교차 등록이 불가능합니다. |
| 서비스 | systemd user 유닛을 쓰고 가능하면 linger를 켭니다. macOS는 launchd, Windows는 예약 작업을 씁니다. systemd가 없는 호스트(PID 1이 tini)는 분리 실행으로 폴백하고 `supervised:false`를 보고합니다. |
| 디스커버리 | 데몬에 `GET /api/v1/fs/repositories`(capability `repoDiscoveryV1`)를 둡니다. 깊이는 4 이하이고 시간, 항목, 결과 수에 상한을 둡니다. `.git`을 만나면 하강을 멈추고 심볼릭 링크는 따라가지 않으며 `node_modules`, `target`, 숨김 디렉터리는 제외합니다. 링크드 워크트리를 표시하고 등록 여부를 조인합니다. UI는 머신 프로젝트 추가 화면에 "발견된 저장소" 목록과 열기 버튼을 둡니다. |
| 보류 | 다른 IDE의 최근 목록 읽기, 재사용 플릿 키, 무인 자동 업데이트. |

## 8. 출처 목록

- http://localhost:4020
- https://buildkite.com/docs/agent/self-hosted/job-acquisition-tokens
- https://buildkite.com/docs/agent/v3/linux
- https://buildkite.com/docs/agent/v3/tokens
- https://buildkite.com/docs/agent/v3/ubuntu
- https://buildkite.com/docs/pipelines/security/clusters/manage
- https://bytexd.com/install-chrome-remote-desktop-headless/
- https://chromium.googlesource.com/chromium/src/+/main/remoting/host/pin_hash.cc
- https://chromium.googlesource.com/chromium/src/+/main/remoting/host/setup/host_starter_base.cc
- https://chromium.googlesource.com/chromium/src/+/main/remoting/host/setup/oauth_host_starter.cc
- https://chromium.googlesource.com/chromium/src/+/main/remoting/host/setup/start_host_main.cc
- https://cli.github.com/manual/gh_codespace_ssh
- https://cloud.google.com/architecture/chrome-desktop-remote-on-compute-engine
- https://code.claude.com/docs/en/authentication.md
- https://code.claude.com/docs/en/claude-code-on-the-web.md
- https://code.claude.com/docs/en/claude-directory.md
- https://code.claude.com/docs/en/remote-control.md
- https://code.claude.com/docs/en/self-hosted-environments-deploy.md
- https://code.claude.com/docs/en/self-hosted-environments-quickstart.md
- https://code.visualstudio.com/docs/remote/linux
- https://code.visualstudio.com/docs/remote/troubleshooting
- https://code.visualstudio.com/docs/remote/tunnels
- https://code.visualstudio.com/docs/remote/vscode-server.md
- https://coder.com/docs/admin/templates/managing-templates/external-workspaces
- https://coder.com/docs/admin/users/sessions-tokens
- https://coder.com/docs/reference/releases.md
- https://community.netdata.cloud/t/automate-netdata-cloud-node-claim-on-instance-deploy/25
- https://community.netdata.cloud/t/is-the-token-for-claiming-new-nodes-different-each-time/1026
- https://cursor.com/docs/cloud-agent/self-hosted.md
- https://cursor.com/docs/cloud-agent/self-hosted/my-machines.md
- https://cursor.com/docs/cloud-agent/self-hosted/pool.md
- https://cursor.com/install
- https://deno.land/install.ps1
- https://developer.apple.com/tutorials/data/documentation/bundleresources/information-property-list/nsdocumentsfolderusagedescription.json
- https://developer.apple.com/tutorials/data/documentation/devicemanagement/privacypreferencespolicycontrol/services-data.dictionary.json
- https://developer.apple.com/tutorials/data/documentation/devicemanagement/servicemanagementmanagedloginitems.json
- https://developer.apple.com/tutorials/data/documentation/servicemanagement/smappservice/register(
- https://developer.apple.com/tutorials/data/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos.json
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/remote-tunnel-permissions/index.md
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/tunnel-permissions/index.md
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel-api/index.md
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel/index.md
- https://developers.cloudflare.com/tunnel/reference/tunnel-tokens/index.md
- https://developers.google.com/identity/protocols/oauth2/limited-input-device.md.txt
- https://developers.openai.com/codex/auth.md
- https://developers.openai.com/codex/cloud/environments.md
- https://developers.openai.com/codex/config-advanced.md
- https://devpod.sh/docs/developing-in-workspaces/inactivity-timeout
- https://devpod.sh/docs/how-it-works/overview
- https://docs.aws.amazon.com/iot/latest/apireference/API_CreateProvisioningClaim.md
- https://docs.aws.amazon.com/iot/latest/developerguide/fleet-provision-api.md
- https://docs.aws.amazon.com/iot/latest/developerguide/iot-provision.md
- https://docs.aws.amazon.com/iot/latest/developerguide/provision-wo-cert.html
- https://docs.cloud.google.com/architecture/chrome-desktop-remote-windows-compute-engine
- https://docs.datadoghq.com/account_management/api-app-keys.md
- https://docs.datadoghq.com/agent/fleet_automation.md
- https://docs.datadoghq.com/agent/supported_platforms/osx.md
- https://docs.datadoghq.com/agent/supported_platforms/windows.md
- https://docs.devin.ai/cli/reference/commands.md
- https://docs.devin.ai/cloud/outposts/partners.md
- https://docs.devin.ai/cloud/outposts/quickstart.md
- https://docs.devin.ai/work-with-devin/local-hands.md
- https://docs.fly.io/flyctl/cmd/fly_auth_login.md
- https://docs.github.com/api/article/body?pathname=/en/rest/actions/self-hosted-runners
- https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners
- https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/remove-runners
- https://docs.github.com/en/actions/reference/runners/self-hosted-runners
- https://docs.github.com/en/actions/reference/security/secure-use
- https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app
- https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps
- https://docs.github.com/en/codespaces/reference/security-in-github-codespaces
- https://docs.github.com/en/rest/actions/self-hosted-runners
- https://docs.gitlab.com/api/runners/
- https://docs.gitlab.com/api/users/
- https://docs.gitlab.com/ci/runners/configure_runners/
- https://docs.gitlab.com/ci/runners/new_creation_workflow/
- https://docs.gitlab.com/ci/runners/runners_scope/
- https://docs.gitlab.com/runner/fleet_scaling/
- https://docs.gitlab.com/runner/register/
- https://docs.gitlab.com/security/tokens/
- https://docs.netbird.io/how-to/approve-peers
- https://docs.netbird.io/how-to/getting-started
- https://docs.netbird.io/how-to/register-machines-using-setup-keys
- https://docs.netbird.io/manage/peers/bootstrap-via-config-file
- https://docs.portainer.io/admin/environments/add/docker/edge
- https://docs.portainer.io/advanced/edge-agent
- https://docs.portainer.io/user/edge/waiting-room.md
- https://docs.rc.fas.harvard.edu/kb/cursor-remote-development/
- https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html
- https://docs.superset.sh/llms.mdx/cli/host-server
- https://docs.superset.sh/llms.mdx/remote-access
- https://docs.superset.sh/llms.mdx/workspaces
- https://docs.syncthing.net/dev/device-ids.html
- https://docs.syncthing.net/rest/cluster-pending-devices-get.html
- https://docs.syncthing.net/users/config.html
- https://docs.syncthing.net/users/faq.html
- https://docs.syncthing.net/users/introducer.html
- https://docs.syncthing.net/users/security.html
- https://docs.termius.com/getting-started/import-existing-hosts.md
- https://docs.termius.com/organize-and-connect-to-hosts/groups-and-tags.md
- https://docs.termius.com/security/encryption-overview.md
- https://docs.warp.dev/terminal/settings/all-settings.md
- https://docs.warp.dev/terminal/warpify/ssh.md
- https://docs.waveterm.dev/connections
- https://docs.windsurf.com/windsurf/advanced
- https://docs.zerotier.com/cli/
- https://docs.zerotier.com/controller/
- https://docs.zerotier.com/start/
- https://docs.zerotier.com/tokens/
- https://forum.cursor.com/t/cursor-ssh-keeps-downloading-vscode-server/158596
- https://forum.cursor.com/t/help-cursor-remote-ssh-fails-with-error-resolving-ssh-authority-this-operation-was-aborted/154281
- https://get.k3s.io
- https://git-fork.com/releasenotes
- https://github.com/BloopAI/vibe-kanban/blob/d5cbb5380fa0b32e98ef9b8d987f63decce4be3a/crates/trusted-key-auth/src/runtime.rs
- https://github.com/BurntSushi/ripgrep/blob/3fce3b5bb0236da2df6d99672afb8a719642eca7/crates/ignore/src/walk.rs
- https://github.com/BurntSushi/walkdir/blob/6fd031c82ba5a4204b4ce6eae73dacb00dc072ec/src/lib.rs
- https://github.com/DataDog/agent-linux-install-script/blob/973102c6145c00744d08f4f1f37af5391d08746f/install_script.sh.template
- https://github.com/DataDog/documentation/blob/9f8f2bea7aceed52e9b44d1c2d8cdacb81bb2213/hugo/content/en/agent/guide/upgrade.md
- https://github.com/Eugeny/tabby/blob/467ce8ebfa467fb88cf6d154c9a5891e3d3647f0/tabby-electron/src/sshImporters.ts
- https://github.com/Eugeny/tabby/blob/467ce8ebfa467fb88cf6d154c9a5891e3d3647f0/tabby-settings/src/services/configSync.service.ts
- https://github.com/Eugeny/tabby/issues/7622
- https://github.com/Homebrew/brew/blob/9811f64e2cccd62fd8a981c1fed5248b9705fea4/Library/Homebrew/services/cli.rb
- https://github.com/Homebrew/brew/blob/9811f64e2cccd62fd8a981c1fed5248b9705fea4/Library/Homebrew/services/system.rb
- https://github.com/Homebrew/brew/commit/77e026b88d35fe08ef9ae3980fb8e821c3637805
- https://github.com/JetBrains/intellij-community/blob/d8c6714c7a3c110c56994d6b0f10814f902100e7/platform/platform-impl/src/com/intellij/ide/RecentProjectsManagerBase.kt
- https://github.com/MisterTea/EternalTerminal/blob/7ee361a75fbb3e0900445b110188996fbe950823/src/terminal/SshSetupHandler.cpp
- https://github.com/Neon6105/intune-scripts-linux/blob/53c87f5820ec34054261aa05b32ede613a2a4f18/install-teamviewer-deb.sh
- https://github.com/NousResearch/hermes-agent/pull/24275
- https://github.com/PowerShell/Win32-OpenSSH/issues/998
- https://github.com/Richards-LLC/cassy/issues/1299
- https://github.com/ThePrimeagen/tmux-sessionizer/blob/7edf8211e36368c29ffc0d2c6d5d2d350b4d729b/tmux-sessionizer
- https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/docs/automate.md
- https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Misc/layoutbin/darwin.svc.sh.template
- https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Misc/layoutbin/systemd.svc.sh.template
- https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Listener/Configuration/ConfigurationManager.cs
- https://github.com/actions/runner/blob/4c989f2f108bb6798e07ea5935bc8bc4aab7de2f/src/Runner.Listener/Configuration/NativeWindowsServiceHelper.cs
- https://github.com/ajeetdsouza/zoxide/blob/a1b102b5a5e43e854a81ab754548fec93a2c1679/README.md
- https://github.com/ajeetdsouza/zoxide/blob/a1b102b5a5e43e854a81ab754548fec93a2c1679/src/db/dir.rs
- https://github.com/ajeetdsouza/zoxide/blob/a1b102b5a5e43e854a81ab754548fec93a2c1679/src/db/stream.rs
- https://github.com/ajeetdsouza/zoxide/wiki/Algorithm
- https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/README.md
- https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/package.json
- https://github.com/alefragnani/vscode-project-manager/blob/aefcf11b422e2a260faeb63a7dfdd6adbf3b1e03/src/autodetect/gitRepositoryDetector.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/README.md
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/authentication.md
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/hq.md
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/docs/security.md
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/ios/VibeTunnel/Services/BonjourDiscoveryService.swift
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/mac/VibeTunnel/Core/Services/NgrokService.swift
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/docs/systemd.md
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/client/services/auth-client.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/middleware/auth.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/routes/repositories.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/server.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/auth-service.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/hq-client.ts
- https://github.com/amantus-ai/vibetunnel/blob/f78324f5a6f5617711581cf5b119651094b2d9bc/web/src/server/services/remote-registry.ts
- https://github.com/amantus-ai/vibetunnel/issues/539
- https://github.com/amantus-ai/vibetunnel/issues/556
- https://github.com/amd/gaia/issues/5578
- https://github.com/amidaware/community-scripts/blob/763e7ed95c799466baa43fa17487eabb3a7ab66a/scripts/Win_Teamviewer_Host_custom_install.ps1
- https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/across_repositories.adoc
- https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/configuration.adoc
- https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/projects.adoc
- https://github.com/bbatsov/projectile/blob/4d2be8680fe445c4ca88177f0ed838051cadfe58/doc/modules/ROOT/pages/usage.adoc
- https://github.com/cli/cli/blob/ed84a97a559956853e4464fee3c0ff4262a0d6ac/pkg/cmd/auth/login/login.go
- https://github.com/cli/cli/blob/trunk/pkg/cmd/codespace/ssh.go
- https://github.com/cli/oauth/blob/7efb8e657cd3a69387b05401be9ce36fa464d3ba/device/device_flow.go
- https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/linux_service.go
- https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/tunnel/login.go
- https://github.com/cloudflare/cloudflared/blob/ee83189f850799d54189d136c1389e13b03d3385/cmd/cloudflared/windows_service.go
- https://github.com/cloudflare/cloudflared/commit/f70adda11cf4c1acf1aa9c1df952baa05269f074
- https://github.com/cloudflare/cloudflared/issues/1539
- https://github.com/coder/code-server/blob/f07405cba33179e6d2058af07ca2be290f19b262/docs/install.md
- https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/agent/agentcontainers/api.go#L441-L540
- https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/apiversion/apiversion.go
- https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/cli/ping.go
- https://github.com/coder/coder/blob/9b8d880d63ed75158433ec764902c3e8d073a993/tailnet/proto/version.go
- https://github.com/d-kimuson/claude-code-viewer/blob/9367cf006be9965c954e6cd36526f2195dce67ea/src/server/core/sync/services/SyncService.ts
- https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/lib/git/rev-parse.ts
- https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/lib/git/worktree.ts
- https://github.com/desktop/desktop/blob/aa8de2c07019efa520f8eb69cb41843d6dbdf9bf/app/src/ui/add-repository/add-existing-repository.tsx
- https://github.com/desktop/desktop/issues/1574
- https://github.com/desktop/desktop/issues/19662
- https://github.com/desktop/desktop/issues/21260
- https://github.com/desktop/desktop/issues/969
- https://github.com/ekzhang/sshx/blob/3604e8ec5ce0e741bd4a7de2e339b980ca8a2d40/crates/sshx-server/src/web/socket.rs
- https://github.com/ekzhang/sshx/blob/3604e8ec5ce0e741bd4a7de2e339b980ca8a2d40/crates/sshx/src/controller.rs
- https://github.com/ekzhang/sshx/issues/146
- https://github.com/fork-dev/Tracker/issues/1094
- https://github.com/fork-dev/Tracker/issues/2067
- https://github.com/fork-dev/Tracker/issues/2512
- https://github.com/fork-dev/Tracker/issues/640
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/RelNotes/2.30.3.adoc
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/config/safe.adoc
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git-rev-parse.adoc
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git-worktree.adoc
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/git.adoc
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/Documentation/gitrepository-layout.adoc
- https://github.com/git/git/blob/7e34d8174dcda3435d1404c9a30bef46afe6c13e/builtin/worktree.c
- https://github.com/google-cloud-sdk-unofficial/google-cloud-sdk/blob/1f04d1ec3a8e0c0b9681fd1efc2ef71aa2d1502c/lib/googlecloudsdk/command_lib/auth/flags.py
- https://github.com/google-cloud-sdk-unofficial/google-cloud-sdk/blob/1f04d1ec3a8e0c0b9681fd1efc2ef71aa2d1502c/lib/googlecloudsdk/core/credentials/flow.py
- https://github.com/grafana/gcx/blob/1a040db2557a7b0a4cf9cf1a9d762d4f1b58cd30/docs/reference/login.md
- https://github.com/gravitational/teleport/blob/1283425b60ec5f60d509ba4c791183d452923ff7/rfd/0184-agent-auto-updates.md
- https://github.com/gravitational/teleport/discussions/47620
- https://github.com/gravitational/teleport/issues/11137
- https://github.com/home-assistant/core/blob/e40dde763b8243aa6a26db0e28926fb8dc633e22/homeassistant/components/onboarding/views.py
- https://github.com/home-assistant/core/pull/169299
- https://github.com/hotio/plex/blob/release/root/etc/s6-overlay/s6-rc.d/init-setup-app/run
- https://github.com/inconshreveable/ngrok/issues/839
- https://github.com/jeanp413/open-remote-ssh/blob/1911064579e9b6162ee90bcd20bf8d1b9d5af552/src/scripts/server-setup.sh
- https://github.com/jellyfin/jellyfin/blob/c7ce6a1b56ee7dcc1896250df0f2f9deaa828a81/Jellyfin.Api/Auth/FirstTimeSetupPolicy/FirstTimeSetupHandler.cs
- https://github.com/jellyfin/jellyfin/issues/17880
- https://github.com/jellyfin/jellyfin/pull/17369
- https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/README.md
- https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/lister/caching_lister.go
- https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/lister/list.go
- https://github.com/joshmedeski/sesh/blob/a145fa1cb7bcc3c8d498e8c0fcd8161e21b4ec1f/namer/git.go
- https://github.com/juanfont/headscale/blob/main/CHANGELOG.md
- https://github.com/juanfont/headscale/blob/main/cmd/headscale/cli/preauthkeys.go
- https://github.com/juanfont/headscale/blob/main/hscontrol/types/preauth_key.go
- https://github.com/junegunn/fzf-git.sh/blob/57ef995e245923ae05721e876c42100a1896ad6c/fzf-git.sh
- https://github.com/k3s-io/k3s/blob/65d71277db7a81c2d498bfbb62e198102889a795/install.sh
- https://github.com/k3s-io/k3s/blob/65d71277db7a81c2d498bfbb62e198102889a795/pkg/clientaccess/token.go
- https://github.com/keycloak/keycloak/blob/f7532e18737e6288bc139acf2bc7373e81af6404/server-spi/src/main/java/org/keycloak/models/OAuth2DeviceConfig.java
- https://github.com/keycloak/keycloak/blob/f7532e18737e6288bc139acf2bc7373e81af6404/services/src/main/java/org/keycloak/protocol/oidc/grants/device/endpoints/DeviceEndpoint.java
- https://github.com/kubernetes/kubernetes/blob/d285472482503c249133440979b51e49949030ea/cmd/kubeadm/app/apis/bootstraptoken/v1/defaults.go
- https://github.com/linuxserver/docker-plex/blob/master/README.md
- https://github.com/loft-sh/devpod-provider-ssh/blob/master/pkg/ssh/ssh.go
- https://github.com/loft-sh/devpod/blob/5a0efcbff6610ab114b421f68a890739a452e66b/pkg/inject/inject.sh
- https://github.com/loft-sh/devpod/blob/5a0efcbff6610ab114b421f68a890739a452e66b/pkg/workspace/list.go
- https://github.com/loft-sh/devpod/issues/776
- https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/faq.md
- https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/ssh.md
- https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/troubleshooting.md
- https://github.com/microsoft/vscode-docs/blob/a03090533810eb02e5b9a2a650e2a3df32192b42/docs/remote/tunnels.md
- https://github.com/microsoft/vscode-remote-release/issues/11136
- https://github.com/microsoft/vscode-remote-release/issues/11721
- https://github.com/microsoft/vscode-remote-release/issues/7778
- https://github.com/microsoft/vscode/blob/9a68ad723966f70414e9bdb325ec1156f51a32b2/cli/src/download_cache.rs
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/auth.rs
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/paths.rs
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_linux.rs
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_macos.rs
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/cli/src/tunnels/service_windows.rs
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/package.json#L3305-L3324
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/extensions/git/src/model.ts
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/platform/storage/electron-main/storageMain.ts
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/browser/abstractFileDialogService.ts
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/browser/simpleFileDialog.ts
- https://github.com/microsoft/vscode/blob/bc656dfadffd56f41fd93e03c550a2c3f3e49c94/src/vs/workbench/services/dialogs/electron-browser/fileDialogService.ts#L67-L74
- https://github.com/microsoft/vscode/blob/main/cli/src/auth.rs
- https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/dev_tunnels.rs
- https://github.com/microsoft/vscode/blob/main/cli/src/tunnels/service_linux.rs
- https://github.com/microsoft/vscode/issues/167741
- https://github.com/microsoft/vscode/issues/173558
- https://github.com/microsoft/vscode/issues/184058
- https://github.com/microsoft/vscode/issues/246907
- https://github.com/microsoft/vscode/issues/329938
- https://github.com/microsoft/vscode/issues/341361
- https://github.com/mxschmitt/action-tmate/blob/master/README.md
- https://github.com/netbirdio/netbird/blob/main/management/server/types/setupkey.go
- https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/packaging/installer/kickstart.sh#L1611-L1628
- https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/src/claim/claim-with-api.c#L198-L337
- https://github.com/netdata/netdata/blob/8db4d108dce1a00c993164e00400e42208d89e89/src/web/api/v2/api_v2_claim.c#L144-L272
- https://github.com/newrelic/newrelic-cli/blob/4c62d7ab4133f0b082bba2ad10daca6fa7211570/internal/install/command.go
- https://github.com/oauth-wg/oauth-cross-device-security/blob/de72489b8b3a2520182e2cb80fedf2010c433ab8/draft-ietf-oauth-cross-device-security.md#L1058-L1213
- https://github.com/openai/codex/blob/f3a90545aed3188ee1e5c119ec89a742de399cdc/codex-rs/login/src/device_code_auth.rs
- https://github.com/openai/codex/blob/f3a90545aed3188ee1e5c119ec89a742de399cdc/codex-rs/rollout/src/list.rs
- https://github.com/ory/hydra/blob/4174065ffb052799890f7480f5360a877a67ffc1/fosite/config_default.go
- https://github.com/owenthereal/upterm/blob/845c706ec0735459ea0d4b0b04df211f7e1a86df/README.md
- https://github.com/owenthereal/upterm/blob/845c706ec0735459ea0d4b0b04df211f7e1a86df/docs/upterm_host.md
- https://github.com/owenthereal/upterm/blob/845c706ec0735459ea0d4b0b04df211f7e1a86df/server/cert.go
- https://github.com/pkkid/python-plexapi/blob/master/plexapi/myplex.py
- https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/README.md
- https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/charts/plex-media-server/values.yaml
- https://github.com/plexinc/pms-docker/blob/abba890d29db2e83321371253a8a3c2be2218196/root/etc/cont-init.d/40-plex-first-run
- https://github.com/plexinc/pms-docker/pull/142
- https://github.com/portainer/agent/blob/develop/README.md
- https://github.com/portainer/agent/blob/develop/edge/key.go
- https://github.com/portainer/portainer/blob/develop/api/http/security/bouncer.go
- https://github.com/portainer/portainer/issues/5473
- https://github.com/radixhound/pilot-manager/issues/8
- https://github.com/rancher/rancher/blob/7b1f21008e6f00afc802492b4d5dcd2544525aa3/pkg/controllers/dashboard/clusterregistrationtoken/status.go
- https://github.com/rancher/rancher/security/advisories/GHSA-23h9-rr79-r3gh
- https://github.com/rancher/rancher/security/advisories/GHSA-7r53-jvhg-9jq4
- https://github.com/raycast/extensions/blob/26ee20de2d66c9d8b18225d9fc4211a2f0689387/extensions/cursor-recent-projects/src/db.ts
- https://github.com/royalapplications/docs/blob/e19b9857e136da464988a0736e279aba34a844fa/src/products/royalts/reference/organization/dynamic-folder.md
- https://github.com/rustdesk/doc.rustdesk.com/blob/56f9310ca2c91f8e94b6a460290d1e94f62e0482/content/self-host/client-configuration/advanced-settings/_index.en.md
- https://github.com/rustdesk/doc.rustdesk.com/blob/56f9310ca2c91f8e94b6a460290d1e94f62e0482/content/self-host/client-deployment/_index.en.md
- https://github.com/rustdesk/doc.rustdesk.com/blob/56f9310ca2c91f8e94b6a460290d1e94f62e0482/content/self-host/rustdesk-server-pro/console/_index.en.md
- https://github.com/rustdesk/rustdesk/blob/828d1101afd2e656eb1c1d18511009bdf5336238/src/core_main.rs
- https://github.com/ryoppippi/ccusage/blob/06347d39a22a1ef61c022e20b22afade702a31d5/rust/adapters/claude/src/paths.rs
- https://github.com/ryoppippi/ccusage/blob/06347d39a22a1ef61c022e20b22afade702a31d5/rust/adapters/codex/src/paths.rs
- https://github.com/sharkdp/fd/blob/9e904639aee44df4507200cf0ed5e8531e120d6f/README.md
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-app/sources/hooks/useConnectTerminal.ts
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-app/sources/sync/machineChoices.ts
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/modules/common/pathSecurity.ts
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/persistence.ts
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-cli/src/ui/auth.ts
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-ios/Sources/HappyCore/NewSession/SessionPlaces.swift
- https://github.com/slopus/happy/blob/1a643badd1b4a5043f5316af0821402c770e55b6/packages/happy-server/sources/app/api/routes/authRoutes.ts
- https://github.com/slopus/happy/issues/1736
- https://github.com/slopus/happy/issues/1830
- https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/docs/site/content/docs/ssh.mdx
- https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/git/worktree-list-reader.ts
- https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/managed-server/install/orcad-remote-install.ts
- https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/project-groups/nested-repo-discovery.ts
- https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/runtime/device-registry.ts
- https://github.com/stablyai/orca/blob/d667e54b02e9be38dc3da26f7c4d4fd24aa4a3b4/src/main/ssh/ssh-host-key-store.ts
- https://github.com/stablyai/orca/issues/9828
- https://github.com/stravu/crystal/blob/1e18e0bc981225f75b5226f82a300fa741970c6f/main/src/services/worktreeManager.ts
- https://github.com/superfly/flyctl/blob/9cca2c596271cdc163784c0dbcc68b420ade6e75/internal/command/auth/webauth/webauth.go
- https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/packages/cli/src/commands/hosts/add/command.ts
- https://github.com/superset-sh/superset/blob/14865a4061fafe18d96e5eff1d06008738facfc3/plans/20261010-add-host-over-ssh.md
- https://github.com/superset-sh/superset/blob/eb9db3ed0e5d6586f67c8d51340bc67f83315c4c/apps/desktop/plans/20260817-remote-onboarding-improvements.md
- https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/logind.conf.xml
- https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/pam_systemd.xml
- https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/man/systemd.exec.xml#L1535-L1560
- https://github.com/systemd/systemd/blob/fa02751cd86f1c7d8683f10ac3f512f677fd0414/src/login/org.freedesktop.login1.policy
- https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/clientupdate/distsign/distsign.go
- https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/cmd/tailscaled/install_darwin.go
- https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/cmd/tailscaled/tailscaled.service
- https://github.com/tailscale/tailscale/blob/f8ad538a3ba0b905c87688c3bc881d20e232a644/scripts/installer.sh
- https://github.com/tailscale/tailscale/blob/main/cmd/containerboot/main.go
- https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/up.go
- https://github.com/tailscale/tailscale/blob/main/scripts/installer.sh
- https://github.com/tailscale/tailscale/blob/main/types/persist/persist.go
- https://github.com/teamviewer/TeamViewerPS/blob/da52a7d1803bfea04ca35f643fc2828f507a0f92/Cmdlets/Private/Resolve-TeamViewerAssignmentErrorCode.ps1
- https://github.com/teamviewer/TeamViewerPS/blob/da52a7d1803bfea04ca35f643fc2828f507a0f92/Cmdlets/Public/Add-TeamViewerAssignment.ps1
- https://github.com/tmate-io/tmate-ssh-server/blob/d7334ee4c3c8036c27fb35c7a24df3a88a15676b/tmate-ssh-daemon.c
- https://github.com/tmate-io/tmate/issues/336
- https://github.com/tpsdev-ai/flair/issues/2040
- https://github.com/warpdotdev/warp/blob/1e6c3ae465d6a925a9b97ae03877aaadd159b8f1/crates/remote_server/src/auth.rs
- https://github.com/warpdotdev/warp/blob/1e6c3ae465d6a925a9b97ae03877aaadd159b8f1/crates/remote_server/src/install_remote_server.sh
- https://github.com/warpdotdev/warp/issues/12449
- https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/conncontroller/conncontroller.go
- https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/remote/connutil.go
- https://github.com/wavetermdev/waveterm/blob/9f1c9670d407be3b93cf090319a63ef30bf54d0e/pkg/wavejwt/wavejwt.go
- https://github.com/wavetermdev/waveterm/issues/2707
- https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/wezterm-client/src/client.rs
- https://github.com/wezterm/wezterm/blob/e2a98e1b8af156988ad0ac778658ac27b60a55fb/wezterm-mux-server-impl/src/pki.rs
- https://github.com/wezterm/wezterm/issues/1972
- https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/README.adoc
- https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/local_repository.go
- https://github.com/x-motemen/ghq/blob/b9273dd116d09b423073b83e7c0d82ef508d985a/worktree.go
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/assets/settings/default.json
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/open_path_prompt/src/open_path_prompt.rs#L71
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/recent_projects/src/remote_servers.rs
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/remote/src/transport/ssh.rs#L1146-L1330
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/workspace/src/persistence.rs#L2159-L2213
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/crates/workspace/src/persistence.rs#L2723-L2775
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/docs/src/remote-development.md
- https://github.com/zed-industries/zed/blob/9e84b066b55bc3609a30dd965075c12255b38b45/docs/src/worktree-trust.md
- https://github.com/zed-industries/zed/issues/34621
- https://goteleport.com/docs/enroll-resources/auto-discovery/servers/ec2-discovery/
- https://goteleport.com/docs/installation/agents/join-token/
- https://goteleport.com/docs/reference/cli/teleport-update.md
- https://goteleport.com/docs/reference/deployment/join-methods/
- https://goteleport.com/docs/upgrading/agent-managed-updates.md
- https://goteleport.com/docs/upgrading/overview.md
- https://goteleport.com/docs/zero-trust-access/authentication/headless/
- https://grafana.com/docs/grafana-cloud/security-and-account-management/authentication-and-permissions/access-policies/create-access-policies/index.md
- https://grafana.com/docs/grafana-cloud/send-data/fleet-management/set-up/onboard-collectors/standalone-alloy/index.md
- https://happy.engineering/docs/security/
- https://headscale.net/stable/ref/registration/
- https://help.gitkraken.com/gitkraken-desktop/open-clone-init/
- https://help.gitkraken.com/gitkraken-desktop/workspaces/
- https://install.datadoghq.com/scripts/install_script_agent7.sh
- https://jira.atlassian.com/browse/SRCTREE-3001
- https://jira.atlassian.com/rest/api/2/issue/SRCTREE-2697?fields=summary,description,comment,status,resolution,created
- https://jira.atlassian.com/rest/api/2/issue/SRCTREE-5100?fields=summary,description,comment,status,created
- https://jira.atlassian.com/rest/api/2/issue/SRCTREE-6860?fields=summary,description,comment,status,created
- https://jira.atlassian.com/rest/api/2/issue/SRCTREE-8009?fields=summary,description,status,created
- https://jules.google/docs/environment/
- https://jules.google/docs/index.md
- https://keith.github.io/xcode-man-pages/launchctl.1.html
- https://keith.github.io/xcode-man-pages/launchd.plist.5.html
- https://learn.chatgpt.com/docs/dots/computers-and-apps.md
- https://learn.chatgpt.com/docs/remote-connections.md
- https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/faq
- https://learn.microsoft.com/en-us/defender-endpoint/controlled-folders
- https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code
- https://learn.microsoft.com/en-us/windows/win32/cfapi/build-a-cloud-file-sync-engine
- https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants
- https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys?accept=text/markdown
- https://login.tailscale.com/a/<hex
- https://man7.org/linux/man-pages/man5/proc_pid_cmdline.5.html
- https://mosh.org/
- https://ngrok.com/docs/api-reference/credentials/create.md
- https://ngrok.com/docs/api-reference/tunnelsessions/list.md
- https://ngrok.com/docs/gateway/agent/authtokens.md
- https://ngrok.com/docs/gateway/agent/config/v3.md
- https://ngrok.com/docs/gateway/agent/service.md
- https://ngrok.com/docs/gateway/device-gateway/fleet-management.md
- https://ngrok.com/docs/iam/service-users.md
- https://ngrok.com/docs/pricing-limits/free-plan-limits.md
- https://pkgs.netbird.io/install.sh
- https://plocate.sesse.net/updatedb.conf.5.html
- https://raw.githubusercontent.com/AsamK/signal-cli/master/man/signal-cli.1.adoc
- https://raw.githubusercontent.com/MicrosoftDocs/PowerShell-Docs/main/reference/7.5/Microsoft.PowerShell.Utility/Invoke-Expression.md
- https://raw.githubusercontent.com/MicrosoftDocs/WSL/main/WSL/systemd.md
- https://raw.githubusercontent.com/MicrosoftDocs/azure-docs-cli/main/docs-ref-conceptual/Latest-version/includes/interactive-login.md
- https://raw.githubusercontent.com/MicrosoftDocs/entra-docs/main/docs/identity-platform/v2-oauth2-device-code.md
- https://raw.githubusercontent.com/MicrosoftDocs/entra-docs/main/docs/identity/conditional-access/concept-authentication-flows.md
- https://raw.githubusercontent.com/MicrosoftDocs/win32/docs/desktop-src/TaskSchd/taskschedulerschema-logontype-principaltype-element.md
- https://raw.githubusercontent.com/MicrosoftDocs/windows-dev-docs/docs/hub/package-manager/winget/install.md
- https://raw.githubusercontent.com/MicrosoftDocs/windowsserverdocs/main/WindowsServerDocs/administration/OpenSSH/openssh-server-configuration.md
- https://raw.githubusercontent.com/MicrosoftDocs/windowsserverdocs/main/WindowsServerDocs/administration/windows-commands/schtasks-create.md
- https://raw.githubusercontent.com/MisterTea/EternalTerminal/master/README.md
- https://raw.githubusercontent.com/MisterTea/EternalTerminal/master/docs/protocol.md
- https://raw.githubusercontent.com/balena-io/balena-cli/e55eda049d6ae23a603700d7f6eee16783c350e3/src/commands/config/generate.ts
- https://raw.githubusercontent.com/balena-io/balena-cli/e55eda049d6ae23a603700d7f6eee16783c350e3/src/utils/config.ts
- https://raw.githubusercontent.com/balena-io/balena-sdk/master/src/models/application.ts
- https://raw.githubusercontent.com/balena-io/docs/a0d2eb7d44495b1bc835db38b2417f52332e71ff/pages/learn/welcome/security.md
- https://raw.githubusercontent.com/buildkite/agent/main/install.sh
- https://raw.githubusercontent.com/cloudflare/cloudflare-docs/production/src/content/docs/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/remote-tunnel-permissions.mdx
- https://raw.githubusercontent.com/cloudflare/cloudflare-docs/production/src/content/partials/cloudflare-one/tunnel/locally-managed/as-a-service/windows.mdx
- https://raw.githubusercontent.com/coder/coder/main/cli/configssh.go
- https://raw.githubusercontent.com/coder/coder/main/cli/login.go
- https://raw.githubusercontent.com/coder/coder/main/coderd/httpmw/workspaceagent.go
- https://raw.githubusercontent.com/coder/coder/main/site/src/pages/CliAuthPage/CliAuthPageView.tsx
- https://raw.githubusercontent.com/coder/terraform-provider-coder/main/docs/resources/agent.md
- https://raw.githubusercontent.com/coder/vscode-coder/main/README.md
- https://raw.githubusercontent.com/denoland/deno_install/master/README.md
- https://raw.githubusercontent.com/github/docs/main/content/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps.md
- https://raw.githubusercontent.com/github/docs/main/content/codespaces/developing-in-a-codespace/opening-an-existing-codespace.md
- https://raw.githubusercontent.com/gravitational/teleport/master/lib/auth/join.go
- https://raw.githubusercontent.com/gravitational/teleport/master/lib/defaults/defaults.go
- https://raw.githubusercontent.com/gravitational/teleport/master/lib/web/join_tokens.go
- https://raw.githubusercontent.com/gravitational/teleport/master/lib/web/scripts/node-join/install.sh
- https://raw.githubusercontent.com/gravitational/teleport/master/tool/tctl/common/token_command.go
- https://raw.githubusercontent.com/imbue-ai/sculptor/main/docs/help/experimental/container_backend.md
- https://raw.githubusercontent.com/jeanp413/open-remote-ssh/master/package.json
- https://raw.githubusercontent.com/k3s-io/docs/main/docs/cli/token.md
- https://raw.githubusercontent.com/kubernetes/design-proposals-archive/main/cluster-lifecycle/bootstrap-discovery.md
- https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/reference/access-authn-authz/bootstrap-tokens.md
- https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/reference/access-authn-authz/kubelet-tls-bootstrapping.md
- https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/reference/setup-tools/kubeadm/kubeadm-join.md
- https://raw.githubusercontent.com/loft-sh/devpod/main/docs/pages/developing-in-workspaces/create-a-workspace.mdx
- https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/02.Overview/14.Device-authentication/docs.md
- https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/03.Client-installation/02.Install-with-Debian-package/docs.md
- https://raw.githubusercontent.com/mendersoftware/mender-docs/f097745761c7ec0bbd1d214cbf5975004701efa6/10.Server-integration/02.Preauthorizing-devices/docs.md
- https://raw.githubusercontent.com/mendersoftware/mender-server/2a52456e94112896bcbf1310a0cabd9440434647/backend/services/deviceauth/devauth/devauth.go
- https://raw.githubusercontent.com/microsoft/vscode-docs/main/docs/remote/tunnels.md
- https://raw.githubusercontent.com/mobile-shell/mosh/master/man/mosh-server.1
- https://raw.githubusercontent.com/netbirdio/docs/ff6170673ef10e3e1adfa5aca80a51758bc76201/src/pages/manage/peers/register-machines-using-setup-keys.mdx
- https://raw.githubusercontent.com/netdata/netdata/master/docs/learn/node-identities.md
- https://raw.githubusercontent.com/netdata/netdata/master/packaging/installer/methods/kickstart.md
- https://raw.githubusercontent.com/netdata/netdata/master/src/claim/README.md
- https://raw.githubusercontent.com/newrelic/docs-website/develop/src/content/docs/apis/intro-apis/new-relic-api-keys.mdx
- https://raw.githubusercontent.com/newrelic/docs-website/develop/src/content/docs/opentelemetry/db360/mysql/cli.mdx
- https://raw.githubusercontent.com/particle-iot/docs/5bb967fe0f95b17dfaa55ffc1628c11d095c2a67/src/content/getting-started/cloud/device-claiming.md
- https://raw.githubusercontent.com/particle-iot/particle-api-js/7bf081abf8ccbb05904895ac1b32d1b45ac6af61/src/Particle.ts
- https://raw.githubusercontent.com/rancher/rancher-docs/main/docs/how-to-guides/new-user-guides/kubernetes-clusters-in-rancher-setup/register-existing-clusters.md
- https://raw.githubusercontent.com/signalapp/Signal-Desktop/main/ts/textsecure/Provisioner.preload.ts
- https://raw.githubusercontent.com/syncthing/syncthing/main/gui/default/index.html
- https://raw.githubusercontent.com/syncthing/syncthing/main/lib/model/model.go
- https://raw.githubusercontent.com/syncthing/syncthing/main/lib/protocol/deviceid.go
- https://raw.githubusercontent.com/tailscale/terraform-provider-tailscale/main/docs/resources/tailnet_key.md
- https://raw.githubusercontent.com/tmux/tmux/master/tmux.1
- https://raw.githubusercontent.com/wezterm/wezterm/e2a98e1b8af156988ad0ac778658ac27b60a55fb/docs/multiplexing.md
- https://raw.githubusercontent.com/wiki/tmux/tmux/Control-Mode.md
- https://storage.googleapis.com/cloud-onboarding/alloy/scripts/install-linux.sh
- https://support.anydesk.com/docs/command-line-interface-for-linux
- https://support.anydesk.com/docs/command-line-interface-for-windows
- https://support.anydesk.com/docs/unattended-access
- https://support.google.com/chrome/a/answer/2799701
- https://support.google.com/chrome/answer/1649523
- https://tailscale.com/docs/concepts/macos-variants.md
- https://tailscale.com/docs/concepts/userspace-networking.md
- https://tailscale.com/docs/features/access-control/auth-keys.md
- https://tailscale.com/docs/features/client/update.md
- https://tailscale.com/docs/features/tailscale-serve.md
- https://tailscale.com/docs/features/tailscale-system-policies.md
- https://tailscale.com/docs/how-to/run-unattended.md
- https://tailscale.com/docs/install/windows/msi.md
- https://tailscale.com/docs/reference/tailscale-cli.md
- https://tailscale.com/install.sh
- https://tailscale.com/kb/1028/key-expiry
- https://tailscale.com/kb/1031/install-linux
- https://tailscale.com/kb/1085/auth-keys
- https://tailscale.com/kb/1099/device-approval
- https://tailscale.com/kb/1226/tailnet-lock
- https://tailscale.com/kb/1241/tailscale-up
- https://vibekanban.com/docs/remote-access.md
- https://vibekanban.com/docs/workspaces/repositories.md
- https://web.archive.org/web/2024id_/https://tmate.io/
- https://wiki.archlinux.org/index.php?title=Systemd/User&action=raw
- https://www.bleepingcomputer.com/news/security/anydesk-says-hackers-breached-its-production-servers-reset-passwords/
- https://www.conductor.build/docs/concepts/git-worktrees.md
- https://www.conductor.build/docs/reference/remote-control.md
- https://www.freedesktop.org/software/systemd/man/latest/loginctl.html
- https://www.gnu.org/software/bash/manual/html_node/Bash-Variables.html
- https://www.ietf.org/archive/id/draft-ietf-oauth-cross-device-security-16.txt
- https://www.jetbrains.com/help/idea/advanced-settings.html
- https://www.jetbrains.com/help/idea/fully-offline-mode.html
- https://www.jetbrains.com/help/idea/remote-development-a.html
- https://www.jetbrains.com/help/idea/remote-development-troubleshooting.html
- https://www.jetbrains.com/help/toolbox-app/jetbrains-gateway-migrations-guide.html
- https://www.microsoft.com/en-us/security/blog/2025/02/13/storm-2372-conducts-device-code-phishing-campaign/
- https://www.portainer.io/how-to/how-to-secure-the-portainer-edge-agent-comms-with-mtls
- https://www.rfc-editor.org/rfc/rfc6749.txt
- https://www.rfc-editor.org/rfc/rfc8628.txt
- https://www.vibekanban.com/blog/shutdown
- https://www.volexity.com/blog/2025/02/13/multiple-russian-threat-actors-targeting-microsoft-device-code-authentication/
- https://youtrack.jetbrains.com/api/issues/IJPL-187560?fields=summary,description
- https://zed.dev/docs/git.md
- https://zed.dev/docs/remote-development.md
- https://zellij.dev/documentation/web-client.html
- https://…

