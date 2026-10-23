# Ferryx relay 프로덕션 호스팅 조사 (2026-10-22)

> 조사 기록: `.omo/ulw-research/20261022-202555-relay-hosting/` (brief, waves/*.md, debate-log.md, measured-*.md, code-facts.md)

## 0. 최종 결정 (2026-10-22, 사용자 결정 — 이 장이 아래 1장의 서울 추천을 대체함)

조사는 "사용자 대부분이 한국"이라는 가정으로 진행했지만, Ferryx는 **전 세계 대상**입니다. 그래서 결정이 다음과 같이 바뀌었습니다.

| 단계 | 운영 위치 | 앞단 | 비고 |
|---|---|---|---|
| **지금** | 집 서버 **omarchy** 유지 | 기존 Cloudflare 터널 그대로 | 아래 "omarchy 운영 보강"을 적용합니다. |
| **다음** | **미국 서부 VM 1대**(Vultr LA `lax` 또는 실리콘밸리 `sjc`, `vc2-1c-1gb` 월 $5) | `relay.ferryx.dev` DNS-only(회색 구름), relay가 직접 TLS(`--tls-domain`, b57f553d) | 서울→LA 131~146ms(WonderNetwork, 조사 h8). 한국과 미주를 둘 다 무난하게 커버합니다. 동부·유럽 수치는 측정하지 않았습니다. |
| **이후** | 사용자가 늘어나는 지역마다 relay 추가(서울, 프랑크푸르트 등) | 지역별 DNS-only | 머신별 `relay_origin`(계정 DB)으로 머신의 홈 지역을 지정합니다. 단, `control_channels`가 프로세스 메모리에 있어서(relay_server.rs:419) 여러 지역 relay가 계정 DB 하나를 공유하는 설계가 먼저 필요합니다. |

**왜 엣지(Cloudflare 프록시)로 해결되지 않는가:** relay는 바이트를 실시간으로 넘겨주는 서버입니다. 엣지가 사용자 근처에서 연결을 받아도 결국 relay 서버까지 갔다 와야 합니다. Argo는 WebSocket을 지원하지 않습니다(3장). 그래서 지연을 줄이는 방법은 지역별 relay와 **직통 P2P 경로**입니다. 세션이 직통으로 넘어가면 relay 위치와 관계없이 지연이 사라집니다(cb1ee6c4 relay→직통 자동 전환).

**omarchy 운영 보강 (지금 단계):** 이번 달 장애 두 건(10-10 ISP 장애, 10-22 디스크 가득 참으로 relay SIGBUS 크래시) 때문에 다음이 필요합니다.
- 디스크 여유 경보(/home 98% 사용 중이었음)
- 외부 업타임 감시
- SQLite를 3.51.3 이상으로 올린 뒤 백업 연결(6장)

**개인정보:** 미국 VM으로 옮기면 개인정보 보호법 제28조의8에 따른 국외 이전 사항을 처리방침에 공개해야 합니다(7장 6번). 옮기기 전에 처리방침부터 고칩니다.

**미국 서부 이전 전에 측정할 것:** KT·SKB·LGU+·모바일에서 Vultr LAX/SJC까지의 지연(저녁 피크 포함)과 실제 월 송신량(relay 바이트 지표).

## 1. 조사 당시 결론 (한국 사용자 가정 — 0장으로 대체됨, 참고용)

**결론: 서울 리전 VM 1대에서 relay를 운영하고, `relay.ferryx.dev`는 Cloudflare 프록시를 끈 DNS-only(회색 구름) 레코드로 둡니다. TLS는 VM에서 직접 처리합니다.**

- **1순위는 Vultr 서울**(`vc2-1c-1gb` 월 $5 또는 `vhp-1c-2gb-amd` 월 $12)입니다.
- **AWS Lightsail 서울**(월 $7, 2TB 포함)이 근소한 차이로 2순위입니다. 아래 7장의 ISP별 실측 결과로 둘 중 하나를 고르면 됩니다.
- **웹사이트 `ferryx.dev`는 지금처럼 Cloudflare에 둡니다.** 정적 페이지라 지연이 문제되지 않습니다.

### 이전 답변과 달라진 점
| 항목 | 이전 답변 | 조사 후 |
|---|---|---|
| 앞단 | 서울 VM 앞에 Cloudflare 프록시(주황 구름) | **프록시 해제(DNS-only)**. 실측 결과 Cloudflare가 이 도메인의 한국 사용자를 서울(ICN)이 아니라 **홍콩(HKG)**에서 처리합니다. 요청 응답 중간값이 133ms로, 직접 연결(19ms)의 7배입니다. 서울 VM을 써도 한국→홍콩→한국으로 돌아가는 경로가 생깁니다. |
| 업체 | Vultr 서울 단일 추천 | **Vultr 서울과 Lightsail 서울이 접전**입니다. 출시 규모의 트래픽에서는 GB당 요금 차이가 결정 요인이 되지 않습니다. |
| 근거 | 검색 도구 호출이 실패한 상태에서 출처를 지어냈습니다(사과드립니다) | 이 문서의 모든 수치에는 실제로 가져온 URL 또는 실측 기록이 붙어 있습니다. |
| 새로 발견 | - | 번들 SQLite 버전이 3.46.0으로, WAL 손상 버그 수정판(3.51.3) 이전입니다. relay는 클라이언트 IP를 TCP 연결에서만 읽습니다. relay 자체에는 TLS가 없습니다. 자세한 내용은 6장에 있습니다. |

## 2. 실측 (이 Mac → 한국 가정용 ISP, 2026-10-22 20:50~21:00 KST)
| 측정 | 결과 | 근거 |
|---|---|---|
| `relay.ferryx.dev`를 처리하는 Cloudflare 데이터센터 | **HKG** (cf-ray 4/4, `/cdn-cgi/trace colo=HKG`) | measured-latency.md |
| 같은 회선에서 `speed.cloudflare.com` | ICN, TCP 연결 24.7ms | 〃 |
| 같은 회선에서 `ferryx.dev` | NRT(도쿄) | 〃 |
| TCP 연결 중간값 | relay.ferryx.dev 101ms / Vultr 서울 19.6ms / AWS 서울 30.1ms / AWS 도쿄 66ms / Vultr 싱가포르 176ms / AWS 싱가포르 217ms | 〃 |
| 요청 응답(keep-alive 20회) | Cloudflare(HKG)+터널 경유 **p50 133ms / p90 270ms**, Tailscale 직접 연결 **p50 19ms / p90 113ms** | 〃 |
| 운영 relay I/O(8.2시간) | rchar 2.58GB + wchar 0.30GB → **월 최대 약 250GB(과대 추정)**. 디스크 읽기가 섞여 있어 실제 네트워크 송신량은 아닙니다. | measured-traffic.md |
| 열린 소켓의 수신/송신 바이트 | 12.4MB / 6.2MB. relay는 받은 만큼 보내므로 들어오는 양과 나가는 양이 비슷합니다. | 〃 |

**한계:** 회선 1개, 시각 1개에서만 쟀습니다. KT·SKB·LGU+와 모바일, 저녁 피크 시간대는 측정하지 않았습니다(7장).

## 3. Cloudflare가 한국 사용자를 어디서 처리하는가
| 사실 | 출처 |
|---|---|
| Cloudflare는 한국 대역폭 비용이 미국 트랜짓 대비 "nearly 30 times"라고 USTR에 제출했습니다(2025-10). | https://torrentfreak.com/images/USTR-NTE-Cloudflare-2026.pdf (제3자 호스팅. 반박 검토 담당은 직접 열람하지 못함) |
| 무료/Pro 존의 한국 사용자가 KT는 LAX, SKB/LGU+는 HKG로 연결된다는 보고가 다수 있습니다. 같은 호스트명을 서울 IP로 강제하면 ICN, 18ms가 나왔습니다. | https://community.cloudflare.com/raw/943456 (#33, #36. 커뮤니티 글이라 일화로만 봅니다) |
| Pro로 올리고 Argo를 켜도 처리 위치가 바뀌지 않았다는 사례가 있습니다. | 같은 스레드 #8 |
| "Argo is not compatible with WebSockets" | https://developers.cloudflare.com/network/websockets/ |
| Cloudflare가 서버를 재시작하면 WebSocket이 끊깁니다. 연결 후에는 WAF 검사도 하지 않습니다. | 같은 페이지 |
| Spectrum의 커스텀 TCP는 Enterprise 전용입니다. | https://developers.cloudflare.com/spectrum/protocols-per-plan/ |
| 2026-09 해저 케이블 장애로 HKG p99가 3~10초까지 늘었습니다. | https://www.cloudflarestatus.com/api/v2/incidents/8wmvkv5jkf15.json |

**판단:** 저렴한 요금제로는 Cloudflare가 한국 사용자를 서울에서 처리하도록 만들 방법이 없습니다. 실시간 터미널 relay는 프록시를 거치지 않는 것이 맞습니다.

## 4. 업체 비교

### 4.1 서울에서 VM을 그대로 운영하는 경우 (현재 코드 그대로 실행 가능)
| 업체 | 최소 사양·월 요금 | 포함 트래픽 | 초과 요금 | 백업 | 출처 |
|---|---|---|---|---|---|
| **Vultr 서울(icn)** | vc2-1c-1gb **$5**(1vCPU/1GB/25GB) · vhp-1c-2gb-amd $12(3TB 포함) | 1TB(vc2) + 계정 전체에 2TB 무료(2022 발표) · **수신 무과금** | **$0.01/GB** | 자동 백업 +20%, 스냅샷 $0.05/GB-월 | https://api.vultr.com/v2/plans?type=vc2 · https://docs.vultr.com/support/platform/billing/what-is-the-bandwidth-overage-rate · https://docs.vultr.com/support/platform/billing/how-is-bandwidth-usage-calculated · https://blogs.vultr.com/Vultr-Announces-Reduced-Bandwidth-Pricing-2-Tb-Of-Free-Monthly-Egress-Free-Ingress-And-Global-Pooling · https://docs.vultr.com/support/platform/billing/how-much-does-it-cost-to-enable-automatic-backups |
| **AWS Lightsail 서울** | $5(0.5GB) · **$7(2TB)** · $12(2GB/3TB) | 번들 포함량. 송신과 수신을 **함께 차감**하지만, 수신 초과분은 과금하지 않음 | **$0.13/GB**(FAQ, 요금 CSV는 $0.126) | 자동 일일 스냅샷 7개 무료, 스냅샷 $0.05/GB-월 | https://aws.amazon.com/lightsail/pricing/ · https://docs.aws.amazon.com/lightsail/latest/userguide/amazon-lightsail-faq-data-transfer-allowance.md · https://docs.aws.amazon.com/lightsail/latest/userguide/amazon-lightsail-bundles.html · https://aws.amazon.com/lightsail/faq/ |
| AWS EC2 서울 | t4g.micro 약 $7.59(제3자 집계) | 없음 | $0.126/GB(10TB까지) | EBS 스냅샷(미확인) | https://a0.awsstatic.com/pricing/1/ec2/pricing-data-transfer-with-regions.min.js · https://ec2.shop/?region=ap-northeast-2 |
| GCP 서울 | e2-micro $7.84(제3자 집계). 무료 티어는 미국 리전만 | Standard 등급 200GiB 무료(서울 제공 여부 미확인) | Premium 등급 한국행 $0.19/GiB(Iowa 표일 가능성 있음, 미확인) | — | https://gcloud-compute.com/asia-northeast3.html · https://cloud.google.com/free/docs/free-cloud-features · https://cloud.google.com/vpc/network-pricing |
| Azure Korea Central | B1s $9.49 / B2ats v2 $8.54 + 고정 IP $3.65 | 100GB 무료 | $0.11~0.12/GB | VM 백업 $10/대 + 스토리지 | https://prices.azure.com/api/retail/prices |
| 가비아 Gen2 | Micro 10,000원 + 공인 IP 4,000원(부가세 별도, 약 $10.6). 스토리지 별도 | 국내 1TB(서버별) / 해외 50GB | 국내 100원/GB, **해외 500원/GB** | — | https://www.gabiacloud.com/virtual_service/ |
| iwinv | vgna_1_n 5,600원(약 $4.2), 공인 IP 포함 | 월 약 600GB(일 20GB) | 미확인 | — | https://docs.iwinv.kr/service/compute/virtual-machine/server-type/general-purpose (2025-05 기준) |
| KINX IXcloud | 인스턴스 가격 미확인 | 1TB(프로젝트 합산) | 80원/GB, 국내·해외 동일 | — | https://www.kinx.net/service/cloud/network/network/ |
| Cafe24 가상서버 | 9,000원(VAT 포함) + 설치비 22,000원 | 300GB | 165원/GB | 고객 책임 | https://hosting.cafe24.com/?controller=new_product_page&page=virtual (리눅스 전 플랜 품절) |
| 네이버 클라우드 / NHN / KT | 가격 미확인(JS 렌더링 페이지를 받아오지 못함) | — | — | — | https://docs.nhncloud.com/en/nhncloud/en/resource-policy/ (NHN은 개인 가입 가능) |

**DERIVED:** 월 250GB(과대 추정)라면 위 업체 전부가 포함량 안에 들어갑니다. 출시 단계에서는 GB당 요금이 차이를 만들지 않습니다. 차이가 생기는 시점은 다음과 같습니다(h2 계산, waves/h2-hyperscalers.md).
| 월 송신량 | Vultr($5 + 3TB 초과분×$0.01) | Lightsail $44(5TB 포함) | EC2 서울 | GCP Standard |
|---|---|---|---|---|
| 1TB | $5 | $44(작은 번들이면 $5~7) | $126 | $68 |
| 5TB | 약 $25 | $44(수신 차감이 있으면 +$315) | $630 | $408 |
| 20TB | 약 $175 | $1,890 | $2,481 | $1,488 |
(Vultr 수치는 `$5 + max(0, E−1TB−2TB풀)×$10/TB`로 계산했습니다. 2TB 풀의 2026년 현행 여부는 미확인입니다.)

### 4.2 서울 밖 또는 무료 티어
| 업체 | 가장 가까운 리전 | 최소 사양·월 요금 | 포함 송신 | 초과 요금 | 서울 기준 RTT | 출처 |
|---|---|---|---|---|---|---|
| Oracle Always Free | 서울(춘천은 A1 제외) | $0. A1 2 OCPU/12GB(2026-06 기준으로 축소됨) | **10TB/월** | 미확인 | 국내 | https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm |
| Linode(Akamai) | 도쿄 2 / 오사카(core) | Nanode $5(1GB) | 1,000GB(글로벌 풀) | **$0.005/GB**, 수신 무료 | 약 31ms(서울→도쿄) | https://api.linode.com/v4/linode/types/g6-nanode-1 · https://api.linode.com/v4/network-transfer/prices · https://techdocs.akamai.com/cloud-computing/docs/network-transfer-usage-and-costs · https://wondernetwork.com/pings/Seoul/Tokyo |
| DigitalOcean | SGP1(노트 기준, 미확인) | $4~6(미확인) | 500~1,000GiB(미확인) | $0.01/GiB, 리전 차등 없음 | 약 73~84ms | https://docs.digitalocean.com/platform/billing/bandwidth/ · https://wondernetwork.com/pings/Seoul/Singapore |
| Hetzner | 싱가포르 SIN1 | CPX02 €7.99 + IPv4 €0.50 | **0.5TB** | €7.40/TB | 약 73~84ms | https://www.hetzner.com/cloud/regular-performance/ · https://docs.hetzner.com/cloud/servers/overview/ |
| OVHcloud / Contabo | 미확인 | 미확인 | 미확인 | 미확인 | 미확인 | 페이지를 받아오지 못함 |

**Oracle을 1순위로 두지 않는 이유**
- 7일 동안 CPU·네트워크·메모리 사용률이 95백분위 기준 20% 미만이면 유휴로 보고 회수합니다("less than 20%", 위 Oracle 문서). 24MB짜리 relay는 이 조건에 그대로 걸립니다.
- 무료 티어에는 SLA가 없습니다(https://www.oracle.com/cloud/free/faq/).
- 계정이 경고 없이 해지됐다는 보고가 여러 해에 걸쳐 있습니다(https://news.ycombinator.com/item?id=23016669 · https://news.ycombinator.com/item?id=34014095 · https://news.ycombinator.com/item?id=43900762).
- 따라서 Oracle은 보조나 스테이징 용도로만 씁니다.

**Linode 도쿄는 가성비가 가장 좋은 차선입니다.** 다만 일본 리전이라 서울 VM보다 왕복 지연이 약 30ms 더 깁니다. 그래서 1·2순위보다 뒤에 둡니다.

### 4.3 PaaS·엣지 (코드를 그대로 돌릴 수 있는지)
| 플랫폼 | 그대로 실행 | 한국에서 가장 가까운 리전 | 제약 | 출처 |
|---|---|---|---|---|
| Fly.io | 가능(머신 1대 + 볼륨) | 도쿄 nrt(서울 icn 없음) | 프록시 keepalive를 바꿀 수 없음. 아시아 송신 약 $0.04/GB(미확인) | https://docs.fly.io/reference/regions.md · https://community.fly.io/t/28809 |
| Railway | 가능 | 싱가포르 | 볼륨을 쓰면 복제본 불가. $0.05/GB | https://docs.railway.com/deployments/regions · https://docs.railway.com/pricing/plans |
| Render | 가능하지만 배포 때마다 중단 | 싱가포르 | 디스크를 쓰면 인스턴스 1개로 고정, 무중단 배포 불가 | https://render.com/docs/disks |
| Koyeb | 불가 | 도쿄·싱가포르 | 아시아 리전에서 볼륨 비활성 | https://app.koyeb.com/v1/catalog/regions?limit=100 |
| Cloud Run | 불가 | 서울 | WebSocket 최대 60분, 세션 고정이 best-effort | https://docs.cloud.google.com/run/docs/triggering/websockets |
| Cloudflare Workers + Durable Objects | **재작성 필요** | apac 힌트만 가능(한국 고정 불가) | 송신 무료, 메시지 최대 32MiB, Hibernation 지원. 다만 위치가 best-effort라 한국 사용자가 다른 곳에서 처리될 수 있음 | https://developers.cloudflare.com/durable-objects/platform/limits/ · https://developers.cloudflare.com/durable-objects/platform/pricing/ · https://developers.cloudflare.com/durable-objects/reference/data-location/ |
| Cloudflare Containers | 불가(영구 디스크 미확인) | — | 한국 송신 $0.05/GB | https://developers.cloudflare.com/containers/pricing/ |

### 4.4 집 서버 omarchy 유지
| 항목 | 내용 | 근거 |
|---|---|---|
| 이번 달 장애 | 10-10 집 ISP 장애, 10-22 디스크 가득 참 → SQLite 쓰기 실패 → relay SIGBUS 크래시 | 메모리 노트 · `journalctl -u ferryx-relay`(12:17:49 status=7/BUS) |
| 앞단 | cloudflared 터널을 거쳐 HKG로 들어오는 경로(위 실측 133ms) | measured-latency.md |
| 판단 | 장기적으로 유료 서비스에는 부적합합니다. 다만 0장의 결정에 따라 **미국 서부 VM으로 옮기기 전까지 운영 호스트로 유지**하며, 운영 보강 항목을 적용합니다. | — |

## 5. 앞단(ingress) 선택
| 방식 | 한국 사용자 경로 | 클라이언트 IP(레이트리밋) | DDoS 방어 | 판단 |
|---|---|---|---|---|
| Cloudflare 터널(현재) | 실측 HKG, p50 133ms | 전부 127.0.0.1로 보임. 레이트리밋 버킷이 하나로 뭉침(2026-09-28 429 실측) | Cloudflare | ✗ |
| Cloudflare 프록시(주황 구름) | 무료/Pro 존은 HKG/NRT/LAX(3장) | 전부 Cloudflare 엣지 IP로 보임. relay가 포워딩 헤더를 신뢰하지 않음 | Cloudflare(연결 후에는 WAF 미검사) | ✗ 지연 |
| **DNS-only + VM에서 TLS** | 서울 VM 직접, 약 20~30ms(실측, 회선 1개) | **실제 클라이언트 IP** | Vultr DDoS 옵션 $10/월, 10Gbps(https://docs.vultr.com/support/platform/billing/how-much-does-ddos-protection-cost) 또는 앱 단에서 연결 수 제한 | ✓ 추천 |
| Spectrum + Argo | — | — | — | Enterprise 전용이라 ✗ |

**DNS-only로 갈 때 주의할 점**
- 서버 IP가 영구히 공개됩니다. 나중에 다시 프록시 뒤로 옮기려면 IP를 바꿔야 합니다.
- L3/4 공격 방어는 업체 옵션에 맡기고, 핸드셰이크·연결 폭주는 relay 쪽에서 연결 수 제한으로 막아야 합니다(반박 검토 C5).

## 6. 이전하기 전에 코드에서 해결할 것 (코드를 직접 읽고 확인)
| 항목 | 사실 | 할 일 |
|---|---|---|
| TLS | `bin/relay.rs`에 TLS/ACME가 없고, `--port 8787`에서 평문 HTTP로 받음 | relay에 rustls+ACME를 넣거나, VM에 Caddy를 두고 신뢰할 프록시 설정을 추가 |
| 클라이언트 IP | `relay_server.rs:3465-3470` `peer_ip()`가 TCP 상대 주소만 사용 | Caddy를 앞에 두면 다시 127.0.0.1로 보임 → PROXY protocol을 쓰거나 relay가 직접 TLS를 처리 |
| 단일 인스턴스 | `relay_server.rs:419` `control_channels`가 프로세스 메모리에 있음 | VM 1대 전제. 수평 확장은 Teleport·Coder처럼 "어느 노드에 붙었는지 기록하는 테이블 + 노드 간 전달" 패턴이 필요(https://raw.githubusercontent.com/gravitational/teleport/master/docs/pages/zero-trust-access/management/proxy-peering.mdx · https://raw.githubusercontent.com/coder/coder/main/docs/admin/networking/high-availability.md) |
| SQLite 버전 | rusqlite 0.32 번들 → **SQLite 3.46.0**. WAL-reset 버그는 3.51.3에서 수정(https://www.sqlite.org/wal.html §11) | Litestream처럼 같은 파일을 여는 두 번째 프로세스를 붙이기 전에 번들 버전을 올림. 또는 relay 안에서 `VACUUM INTO`로 백업 |
| 디스크 가득 참 → SIGBUS | WAL의 `-shm` 파일은 `mmap_size`와 관계없이 mmap됨(https://www.sqlite.org/wal.html §7 · https://www.sqlite.org/mmap.html) | 디스크 여유 경보, 다른 서버에서 업타임 감시(Uptime Kuma, https://raw.githubusercontent.com/louislam/uptime-kuma/master/README.md) |
| 백업 | Litestream v0.5.17(2026-08-31), S3 복제가 1초 단위(https://github.com/benbjohnson/litestream/releases/tag/v0.5.17). R2는 10GB까지 무료, 송신 무료(https://developers.cloudflare.com/r2/pricing/index.md) | SQLite를 올린 뒤 Litestream→R2 연결. VM 스냅샷도 병행 |
| 무중단 재시작 | systemd fd store로 소켓을 유지한 채 재시작 가능(https://raw.githubusercontent.com/systemd/systemd/main/docs/FILE_DESCRIPTOR_STORE.md) | 우선은 클라이언트 재접속으로 충분. 필요해지면 도입 |

## 7. 남은 불확실성 (결정 전에 확인할 것)
1. **ISP별 지연:** KT·SKB·LGU+와 모바일에서, 저녁 피크 시간대에 Vultr 서울(`sel-kor-ping.vultr.com`)과 Lightsail 서울을 각각 측정해야 합니다. 이번 실측은 회선 1개뿐입니다. 이 결과로 1·2순위를 확정합니다.
2. **실제 송신량:** 지금 수치는 rchar/wchar 기반 과대 추정입니다. relay 안에 바이트 지표를 넣거나 nft 포트 카운터로 다시 재야 합니다.
3. **Vultr 계정 단위 2TB 무료 풀:** 2026년에도 유효한지 미확인입니다(2022 발표만 확인). SLA 문서와 장애 이력은 Cloudflare 봇 차단 때문에 받아오지 못했습니다.
4. **국내 클라우드 가격:** 네이버·NHN·KT는 미확인입니다. KINX가 국내 중립 IX 운영사라 국내 피어링에 유리할 수 있지만 측정하지 않았습니다.
5. **Cloudflare Enterprise "Network Prioritization":** 이 기능이 ICN 처리를 보장하는지 미확인입니다(영업 문의 필요).
6. **개인정보:** 서울 리전이면 국외 이전 조항(개인정보 보호법 제28조의8)을 피할 수 있다는 것은 해석입니다. 도쿄·싱가포르로 간다면 처리방침에 국외 이전 내용을 공개하면 됩니다(https://www.law.go.kr/LSW/lsSideInfoP.do?lsiSeq=283839&joNo=0028&joBrNo=08&docCls=jo&urlMode=lsScJoRltInfoR). 국내대리인 지정(매출 1조원 또는 일평균 100만 명)과 ISMS 의무(매출 100억원 또는 일평균 100만 명)는 해당하지 않습니다(https://www.law.go.kr/LSW/lsSideInfoP.do?lsiSeq=289537&joNo=0032&joBrNo=03&docCls=jo&urlMode=lsScJoRltInfoR · https://www.law.go.kr/LSW/lsSideInfoP.do?lsiSeq=290483&joNo=0049&joBrNo=00&docCls=jo&urlMode=lsScJoRltInfoR).

## 8. 반박 검토 결과 (debate-log.md)
| 주장 | 판정 | 반영한 수정 |
|---|---|---|
| C1 Cloudflare 무료/Pro 존이 한국 사용자를 서울에서 처리하지 않음 | 유지 | "회선 1개 실측"으로 범위를 좁혀 표기 |
| C2 Argo로는 해결 안 됨 | 유지 | Spectrum TCP+Argo는 Enterprise 전용임을 추가 |
| C3 트래픽 요금 | 약화 | Lightsail은 수신을 포함량에서 차감하지만 수신 초과분은 과금하지 않음 |
| C4 트래픽 실측 | 약화 | rchar/wchar는 디스크 I/O가 섞인 과대 추정 → 결론(가격보다 지연·안정성 우선)은 유지 |
| C5 DNS-only의 DDoS 위험 | 유지 | 앱 단 연결 수 제한 필요, IP 공개는 되돌릴 수 없음 |
| C6 SQLite SIGBUS | 약화 | WAL-reset 버그(3.51.3) 추가 → 6장에 반영 |
| C7 서울 지연 20~30ms | 약화 | 회선 1개 표본 → 7장 1번 |
| 결론 | **유지**(가장 강한 대안은 Lightsail 서울) | 1장에 반영 |

## 9. 방법
- 조사 담당 8개(Vultr, 하이퍼스케일러, 국내 클라우드, Oracle/저가 VPS, PaaS·엣지, Cloudflare 앞단, 운영 구조, 법규·지연·트래픽)와 반박 검토 1개를 모두 claude-opus-5-5로 돌렸습니다.
- 직접 측정한 것은 지연, Cloudflare 처리 위치, 소켓 바이트, 코드입니다.
- "미확인"으로 표시한 항목은 근거로 쓰지 않았습니다.
- 각 담당의 원문 응답은 `.omo/ulw-research/20261022-202555-relay-hosting/waves/`에 있습니다.
