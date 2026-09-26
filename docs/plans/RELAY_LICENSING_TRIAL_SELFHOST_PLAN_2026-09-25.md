# Ferryx 상용화 계획 — LS(라이선스 서버) · 30일 무료체험 · 셀프호스팅 영구무료

> 상태: 확정(레인 6개 설계 병합 · 외부 리뷰 7라운드 통과) · 작성 2026-09-25 · 범위: 프론트/백엔드/앱/모바일/결제/이메일/운영

## 0. 문서 목적과 요구사항
사용자 요구 3축:
1. **LS(라이선스 서버)** — 회원가입(계정) 게이트를 통해 원격/릴레이 기능을 라이선스로 통제
2. **30일 무료체험** — **첫 기기 등록(/machines/enroll) 후** 30일, 종료 시 원격 기능 제한(로컬 무제한 유지)
3. **셀프호스팅 영구무료** — 자기 서버/로컬 사용은 계정·라이선스 없이 영구 무료

"필요한 모든 사항"의 범위: 라이선스 코어, 백엔드 강제, 결제(MoR), 이메일, 데스크톱 앱, 원격 웹, 모바일, 데이터스토어, 운영/배포, 보안/남용, 단계별 실행계획.

## 1. 현재 시스템 실측 (file:line)

### 1.1 계정 서비스 (relay 전용)
| 사실 | 위치 |
|---|---|
| 계정 라우터는 **relay 바이너리에만** 마운트된다 | `src-tauri/src/bin/relay.rs:149` (`AccountState::new(&data_dir, &origin, mailer)`) |
| 계정 API 라우트 14종 (public-key, login/request·consume, health, device/request·poll·approve, logout, machines, machines/enroll …) | `src-tauri/src/account/service.rs:1114-1132` |
| 데스크톱 앱 내장 서버에는 계정 라우터가 없다 → 앱 오리진 `/api/account/v1/*` 는 404 | 실측: `http://127.0.0.1:43821/api/account/v1/health` → 404, `login/request` → 404 |
| 클라이언트는 계정 API 오리진을 자동 해석한다(헬스 프로브 → 실패 시 기본 오리진 폴백) | `ui/src/remote/accountSession.ts:171`(DEFAULT_ACCOUNT_ORIGIN), `:197`(resolveAccountOrigin) |

### 1.2 데이터 스토어
| 사실 | 위치 |
|---|---|
| `AccountStore` = users / sessions / login_codes / enrollment_codes / machines / grants / device_auths | `src-tauri/src/account/store.rs:132` |
| JSON 파일 로드/저장 + 디렉터리 락 | `store.rs:150`(load), `:161`(save), `:195`(lock_account_dir) |
| TTL·한도 상수: 로그인 코드 600초, 세션 30일, 등록 코드 600초, 시간당 로그인 요청 5회, 본문 4KiB | `store.rs:10-14` |
| 운영 실데이터(omarchy): users=6, machines=3, loginCodes=1 | 실측: `~/.ferryx/account-data/account-store.json` |

### 1.3 서명·검증 프리미티브 (라이선스 재사용 대상)
| 사실 | 위치 |
|---|---|
| Ed25519 검증(계정 서명 grant) | `src-tauri/src/remote/account_grants.rs:8`(ed25519_dalek), `:26`(submission_signing_input), `:38`(verify_grant_signature) |
| 계정 공개키는 relay 운영자가 고정(pin) | `account_grants.rs:36-38` 주석 + relay drop-in `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY` |
| 공개키 조회 라우트 | `account/service.rs:1114` (`GET /api/account/v1/public-key`) |

### 1.4 기기·릴레이 경로
| 사실 | 위치 |
|---|---|
| 릴레이 라우트: pair/exchange, attach/session, `/host/{machine_id}/api/v1/{*path}`, tunnel/control·data·client·opaque, install.sh·install.ps1, download/* | `src-tauri/src/remote/relay_server.rs:2182-2224` |
| 기기 등록 인증이 **정적 공유 비밀**(env `FERRYX_RELAY_MACHINE_TOKENS`) 기반 | `relay_server.rs:286`(machine_tokens 필드), `:348-389`(생성자) |
| 데스크톱 데몬 롤링 핸드오버(무손실 세션 이전) | `src-tauri/src/daemon/protocol.rs:1592`(upgradeBinary), `:1622`(UpgradeScheduled), `daemon/client.rs:364` |

### 1.5 이메일
| 사실 | 위치 |
|---|---|
| Mailer 트레이트: `send_magic_link(to, url)` | `src-tauri/src/account/mailer.rs:31` |
| Resend 구현(페이로드 생성 → POST `https://api.resend.com/emails`) | `mailer.rs:147`(resend_payload), `:161`(ResendMailer), `:183`(POST) |
| 발신 주소 등 환경변수 | relay drop-in `FERRYX_MAIL_FROM` 등 |

### 1.6 배포·릴리스
| 사실 | 위치 |
|---|---|
| 릴리스는 **로컬 전용**(호스팅 CI가 릴리스 산출물을 만들 수 없음) | `AGENTS.md:104`, `scripts/release-workflow-policy.mjs` |
| CLI 설치 스크립트(POSIX/Windows) + 아티팩트 서빙 | `scripts/install.sh`, `scripts/install.ps1`, `relay_server.rs:2221-2224` |
| relay 서비스는 systemd 유닛 + drop-in(계정 공개키·메일·기기토큰·RUST_LOG) | 운영 호스트 실측(omarchy `/etc/systemd/system/ferryx-relay.service.d/`) |

### 1.7 결제/요금제
| 사실 | 위치 |
|---|---|
| **결제·구독·쿼터 코드가 저장소에 전무**(이번 세션에서 `subscription|billing|stripe|paddle|entitlement|quota` 검색 0건) | 해당 없음 — 신규 구현 대상 |

### 1.8 클라이언트 표면
| 사실 | 위치 |
|---|---|
| 원격 웹/모바일 UI | `ui/src/remote/` (RemoteApp.tsx, AccountLoginPage.tsx, AccountMachinesPage.tsx, MobileHostDrawer.tsx, chat/) |
| 데스크톱 설정 UI | `ui/src/components/settings/` |
| 앱은 계정 없이 로컬에서 동작(터미널·워크트리·LAN 페어링) | `bin/relay.rs:149`가 유일한 계정 마운트 지점이라는 사실에서 유도 |

## 2. 세 가지 라이선스 모드 (요구사항 → 설계 계약)

| 모드 | 획득 경로 | 만료 | 계정 필요 | 폰홈(네트워크) | 릴레이 사용 |
|---|---|---|---|---|---|
| **TRIAL** | 첫 성공 `/machines/enroll` 시 발급 | 최초 활성화 + **30일** | 필요 | 최소(만료 확인용 짧은 라이선스) | 허용(체험 한도 내) |
| **PAID** | 결제 구독 | 기간제(갱신) | 필요 | 필요(갱신·상태 동기화) | 허용(플랜 한도) |
| **SELF-HOST** | 무료 영구 라이선스(또는 없음) | **영구** | **불필요** | **없음** | 자체 relay만 |

**핵심 원칙 (설계 불변식)**
1. **로컬은 절대 게이트하지 않는다.** 터미널·워크트리·LAN 페어링·데스크톱 앱 사용은 계정/라이선스 없이 100% 동작한다(현재 구조 유지 — 계정 서비스는 relay에만 마운트, `bin/relay.rs:149`).
2. **게이트는 서버에서 강제한다.** UI 숨김/뱃지는 보조 수단일 뿐이며, 강제 지점은 relay의 기기 등록·attach·프록시 경로다(`relay_server.rs:2182-2224`).
3. **셀프호스팅은 영구 무료이며, `FERRYX_DEPLOYMENT_MODE=selfhost`를 명시한 배포에서만 무계정 경로가 열린다**(설정 부재로 자동 개방 금지 — sol-6 B1/신규B 수용). **기존 selfhost 배포는 업그레이드 전에 이 변수를 서비스/drop-in에 명시적으로 배포해야 한다**(마이그레이션 절차: §7).
4. **배포 모드는 추론하지 않고 명시한다(신규 — sol-6 B1).** `FERRYX_DEPLOYMENT_MODE=commercial|selfhost`를 **필수**로 두고, **기본값을 두지 않는다**. `commercial`은 기동 시 계정/LS 키와 데이터스토어 연결을 검증하고, 검증 실패 시 **유료 표면(신규 기기 등록·신규 원격 attach)에 대해 fail-closed**한다. `selfhost`만 무계정 경로를 허용한다. → "설정 누락"이 상용 relay를 무제한 self-host로 강등시키는 경로를 차단한다.
5. **실패는 열화(degrade)로, 파괴로 하지 않는다.** 라이선스 만료·결제 실패는 원격 접속을 제한하되 로컬 세션·데이터를 잃게 하지 않는다.

## 2.1 상태 머신 (사용자 1명 기준)
```
가입(no_license) ──▶ TRIAL_ACTIVE(30일) ──▶ TRIAL_ENDING(D-7, D-1 알림)
                          │                        │
                          │ 결제                    │ 미결제
                          ▼                        ▼
                    PAID_ACTIVE ──결제실패──▶ PAST_DUE(유예) ──▶ PAID_SUSPENDED(원격 차단, 로컬 유지)
                          │                        │
                          │ 해지                    │ 재결제
                          ▼                        ▼
                 CANCELED_AT_PERIOD_END ──기간종료──▶ (원격 차단)
                          │
                          │ 환불/차지백(웹훅)
                          ▼
                 REFUNDED / CHARGEBACK ──▶ entitlement 즉시 제거 + revocation epoch 증가 (기존 세션 유지, 신규 차단)
```
- **SELF-HOST**는 이 상태 머신 밖에 있다(라이선스가 영구이며 상태 전이가 없음).

## 2.2 요구사항 추적표 (요구 → 섹션)
| 요구 | 설계 섹션 | 강제 지점 |
|---|---|---|
| LS(라이선스 서버) | §3.1 | relay + 계정 서비스 |
| 30일 무료체험 | §3.1(시작/만료), §3.4(알림 메일), §3.5(체험 UI) | relay(서버 판정) + 클라이언트(표시) |
| 셀프호스팅 영구무료 | §3.1(영구 라이선스), §3.6(운영), §3.5(UX) | 없음(강제 지점 없음 = 설계 목표) |
| 결제 | §3.3 | MoR 웹훅 → entitlement |
| 이메일 | §3.4 | Resend(`account/mailer.rs`) |
| 모바일 | §3.5 | 원격 클라이언트 |
| 데이터/마이그레이션 | §3.6 | `AccountStore` → SQLite |
| 보안/남용 | §3.6 | 전 구간 |

## 3. 설계 (주제별)

### 3.1 LS(라이선스 서버) 코어
- 라이선스 문서 형식·서명·키 관리·모드 3종 인코딩 → 상세: `.omo/plans/relay-licensing-sections/01-ls-core.md`
- 핵심 계약(요약): 단일 서명 JSON 라이선스. **필수 필드(sol-6 B4 수용)**: `license_id`, `subject_account_id`, `deployment_id`, `machine_binding`, `nonce`, `mode`, `issued_at`, `not_before`, `expires_at`(null=영구), `entitlements`, `key_id`, `signature`. 서명 입력은 **도메인 분리 프리픽스**를 포함해 기존 grant 서명과 교차 사용을 불가능하게 한다. 검증은 `key_id`로 인덱싱한 **키링**을 사용하며 회전은 **overlap → retirement** 절차를 따른다. 공개키는 relay/클라이언트가 **핀 고정**(현행 `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY`).
- **폐기/캐시 적용 대상 분리(충돌 제거, sol-6 B4/R3-4 수용)**: 세 수치는 **서로 다른 대상**에 적용되며 하나의 함수가 두 정책을 겸하지 않는다.
  * **원격 접근 권한**: 서버 판정. 온라인 폐기 epoch 반영 **≤15분**. 오프라인 클라이언트는 TTL **≤24시간** 내 재확인.
  * **로컬 기능**: 무기한 유지(라이선스와 무관).
  * `isRemoteAttachAllowed()`는 **캐시 TTL을 넘기면 무조건 false**(서버 재확인 전까지 fail-closed) — 즉 grace 14일은 이 함수에 적용되지 않는다. grace는 **라이선스 파일을 못 받은 상태에서 UI 표시를 유지**하는 용도로만 쓰인다(원격 허용과 무관).


<details>
<summary>레인 상세 원문 (01-ls-core.md)</summary>

# 01 — LS(license server) 코어

> Ferryx 상용 모델 계획의 LS 코어 섹션. "현재 시스템" 주장은 전부 `파일:줄`로 인용했고 인용 줄은 이 세션에서 직접 읽어 확인했다.
> 신규 설계·신규 파일은 `(신규)`로 표시한다.

#### 0. 기준선 — 지금 있는 것 / 없는 것

- 계정 서비스 라우트는 한 곳에 모여 있다: `public-key` `service.rs:1114`, `login/request` `:1115`, `health` `:1116`,
  `login/consume` `:1117`, `device/request` `:1118`, `device/poll` `:1119`, `device/approve` `:1120`, `logout` `:1121`,
  `enrollment-codes` `:1123`, `machines` `:1126`, `machines/enroll/challenge` `:1128`, `machines/enroll` `:1131`,
  `machines/{machine_record_id}/grants` `:1133`, 본문 상한 `:1136` (`src-tauri/src/account/service.rs`).
- 상태는 단일 JSON + 파일 락: `AccountStore { users, sessions, login_codes, enrollment_codes, machines, grants, device_auths }`
  (`account/store.rs:132-147`), 경로 `account-store.json`(`store.rs:187`), 락 `lock_account_dir`(`store.rs:195`),
  원자적 0600 쓰기 `write_private_json`(`store.rs:216`).
- 서명 프리미티브는 이미 있다: Ed25519(`remote/account_grants.rs:8`), 도메인 분리 상수
  `GRANT_SUBMISSION_DOMAIN = b"ferryx relay grant submission v1"`(`:14`), `\0` 구분 바이트열 직렬화
  `submission_signing_input`(`:26`), 검증 `verify_grant_signature`(`:38`). 핀이 없거나 서명이 틀리면 403 fail-closed(`:38-45`).
- 계정 서명키는 데이터 디렉터리에 지연 생성·영속(`service.rs:69`): 파일 `account-signing-key.json`(`store.rs:112`),
  레코드 `{ public_key, private_key }`(`store.rs:116`), 공개키는 base64 응답(`service.rs:266`, DTO `service.rs:259-263`).
- 배포는 단일 바이너리: `relay_router_with_account`(`remote/relay_server.rs:2167`)가 끝에서
  `router.merge(crate::account::service::router(account))`(`:2233`). 진입점 `bin/relay.rs`: origin `:126`,
  데이터 디렉터리 `:133`, 계정 공개키 핀 `:44`, 라우터 조립 `:153`. 빌드 산출물 `ferryx-cli`/`ferryx-account`/`ferryx-relay`
  (`Cargo.toml:20-22`, `:24-26`, `:28-30`).
- **라이선스 코드는 없다(신규)**: `pub mod license;` 검색 0건. `account/mod.rs` 모듈은
  `enroll_client, mailer, offer_sink, origin, service, store` 뿐이다.
- 기존 문서가 낡았다: `docs/account-service.md:12-13`은 계정 서비스를 "별도 프로세스"라 하지만 현재는 릴레이에 병합됐다(§7 P6에서 정정).

#### 1. 하나의 서명 문서로 세 모드를 인코딩한다

| mode | 대상 | 만료 | 기기 한도 | 폰홈 | 발급 경로 |
|---|---|---|---|---|---|
| `trial` | 계정 사용자 | 최초 활성화 +30일 | 1 | 활성화 1회 + 갱신 시 | `POST /api/account/v1/license/activate` |
| `paid` | 계정/조직 | 기간제(30일 슬라이스, 갱신) | 엔타이틀먼트 | 갱신 시 | `POST /api/account/v1/license/renew` |
| `selfhost` | 누구나 | 없음(`expires_at: null`) | 무제한(`null`) | 없음 | `POST /api/account/v1/license/selfhost` 또는 운영자 오프라인 발급 |

세 모드는 **동일 스키마 1개**를 쓴다. 검증기 1개·게이트 1개·포맷 1개 — 모드 차이는 필드 값으로만 표현한다.
모드 전용 필드를 만들지 않는 이유: 검증 경로가 갈라지면 우회면도 같이 늘어난다.

##### 1.1 `LicenseDocument` 필드와 타입

표기는 **snake_case**다. `src-tauri/AGENTS.md:23`의 wire 관례는 camelCase지만 이 문서는 IPC DTO가 아니라 **서명 대상
정본(canonical artifact)**이라 Rust 필드명과 1:1로 둔다 — canonical 입력의 필드 순서와 JSON 키가 어긋날 여지를 없앤다.

| 필드 | 타입 | null | 의미 |
|---|---|---|---|
| `schema` | string | ✗ | `"ferryx.license.v1"` 고정. 다른 값은 거부 |
| `license_id` | string(UUIDv4) | ✗ | 발급 단위 식별자. 재활성화해도 유지 |
| `subject.kind` | `"user"\|"org"` | ✗ | 발급 대상 종류 |
| `subject.id` | string | ✗ | `usr_*`(`store.rs:40-46` UserRecord.user_id 형식) 또는 `org_*` |
| `subject.email` | string | ✓ | 청구/안내용. 서명 대상, 게이트 판정 미사용 |
| `mode` | enum 3종 | ✗ | 위 표 |
| `issued_at` / `not_before` | u64 (unix sec) | ✗ | 서버 발급 시각. 클라이언트 시각을 쓰지 않는다 |
| `expires_at` | u64 | ✓ | `null` = 영구(selfhost 전용, mode와 교차 확인) |
| `grace_seconds` | u32 | ✗ | 만료 후 degraded 구간. `0`이면 즉시 제한 |
| `entitlements.max_machines`/`max_devices` | u32 | ✓ | `null` = 무제한 |
| `entitlements.relay_minutes_per_day` | u32 | ✓ | `null` = 무제한, `0` = 공식 릴레이 불가 |
| `entitlements.features` | string[] | ✗ | 정렬·중복제거된 기능 플래그 |
| `machine_limit` / `device_limit` | u32 | ✓ | **정규화 사본**: 각각 `entitlements.max_machines`/`max_devices`와 동일해야 함(§5) |
| `features` | string[] | ✗ | 정규화 사본: `entitlements.features`와 동일 |
| `binding.machine_fingerprint` | string `sha256:<hex16>` | ✓ | `null`이면 기기 비구속 (§3.2) |
| `binding.installation_id` | string(UUID) | ✓ | UI 저장키 `ferryx.remote.installation-id`(`ui/src/lib/storageKeys.ts:18`)와 같은 값 |
| `key_id` | string `acct-<yyyy-mm>-<8hex>` | ✗ | 서명키 식별자 (§2) |
| `signature` | string(base64) | ✗ | Ed25519 서명. canonical 입력에서 제외 |

##### 1.2 canonical 바이트 직렬화 (기존 패턴 재사용)

`account_grants.rs:26-33`의 `\0` 구분 방식과 도메인 분리(`:14`)를 따른다. JSON 원문을 서명하지 않는 이유는 순서·공백·
이스케이프 차이로 서명이 깨지기 때문이다(정본 JSON 정규화 라이브러리는 이 크레이트에 없다).

```
LICENSE_DOCUMENT_DOMAIN = b"ferryx license document v1"                       // (신규)
input = DOMAIN + schema + 0x00 + license_id + 0x00 + subject.kind + 0x00 + subject.id + 0x00 + opt(subject.email) + 0x00
      + mode + 0x00 + dec(issued_at) + 0x00 + dec(not_before) + 0x00 + opt_dec(expires_at) + 0x00 + dec(grace_seconds) + 0x00
      + opt_dec(max_machines) + 0x00 + opt_dec(max_devices) + 0x00 + opt_dec(relay_minutes_per_day) + 0x00
      + entitlements.features.join(",") + 0x00
      + opt_dec(machine_limit) + 0x00 + opt_dec(device_limit) + 0x00 + features.join(",") + 0x00
      + opt(binding.machine_fingerprint) + 0x00 + opt(binding.installation_id) + 0x00 + key_id
```

규칙: `opt/opt_dec`는 `null`을 `-`(0x2D)로 인코딩한다. 모든 필드는 NUL 없는 ASCII로 검증한다(파서가 거부).
`features`는 사전순 정렬·중복 제거 후 결합한다. `signature`는 입력에서 제외한다.

##### 1.3 문서 예시 (trial) — 파싱 가능한 JSON

```json
{
  "schema": "ferryx.license.v1",
  "license_id": "6f0b1c2e-3d4a-4b5c-8d9e-0a1b2c3d4e5f",
  "subject": { "kind": "user", "id": "usr_83bcb9ff65f1ff84", "email": "owner@example.com" },
  "mode": "trial",
  "issued_at": 1789000000,
  "not_before": 1789000000,
  "expires_at": 1791592000,
  "grace_seconds": 259200,
  "entitlements": { "max_machines": 1, "max_devices": 2, "relay_minutes_per_day": 120, "features": ["local", "relay", "remote_attach"] },
  "machine_limit": 1,
  "device_limit": 2,
  "features": ["local", "relay", "remote_attach"],
  "binding": { "machine_fingerprint": "sha256:0f1e2d3c4b5a6978", "installation_id": "4b0d1f2a-9c8e-4d7b-9a6f-1e2d3c4b5a69" },
  "key_id": "acct-2026-09-a1b2c3d4",
  "signature": "T3RoZXIgZmllbGRzIGlkZW50aWNhbCwgZW8gc2lnbmF0dXJlIGNoYW5nZXMgYnl0ZXM="
}
```

`selfhost`는 같은 문서에서 아래 키만 달라진다(나머지 필드는 동일 스키마로 반드시 채운다):

```json
{"mode": "selfhost", "expires_at": null, "grace_seconds": 0, "entitlements": {"max_machines": null, "max_devices": null, "relay_minutes_per_day": 0, "features": ["local", "selfhost_relay"]}, "machine_limit": null, "device_limit": null, "features": ["local", "selfhost_relay"], "binding": {"machine_fingerprint": null, "installation_id": null}}
```

#### 2. 키 관리

- **현재**: 서명키는 데이터 디렉터리에 지연 생성(`service.rs:69`). 경로 `signing_key_path()`(`store.rs:191`, `store.rs:112`),
  레코드 `AccountSigningKeyRecord { public_key, private_key }`(`store.rs:116`, Debug는 REDACTED `store.rs:121-130`).
  릴레이는 공개키를 **핀**한다 — `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY`(`bin/relay.rs:44`), `--account-public-key`로 대체 가능,
  검증 실패는 403(`account_grants.rs:38-45`).
- **key_id(신규)**: `"acct-<yyyy-mm>-" + hex(sha256(공개키 32바이트)[0..4])`. 문서가 자기 서명키를 지목하므로 로테이션 후에도
  옛 문서를 올바른 키로 검증할 수 있다.
- **로테이션(신규)**: 키 파일을 디렉터리로 승격 — `account-signing-keys/<key_id>.json` + `account-signing-keys/active`
  (활성 key_id 포인터). 발급은 `active`로만, 검증은 **수용 집합 전체**로. 수용 집합은 `FERRYX_ACCOUNT_TRUSTED_KEYS`
  (콤마 구분 `key_id:base64` 목록, 신규)로 배포하고, 기존 단일 변수(`bin/relay.rs:44`)는 `acct-legacy:<값>` 1개짜리 집합으로
  계속 동작한다(하위 호환).
- **클라이언트 핀(신규)**: `~/.ferryx/remote/license-trust.json`(0600, `write_private_json` 재사용 `store.rs:216`)에 캐시.
  새 키는 라이선스 갱신 응답에 실어 배포한다(별도 채널 불필요).
- **오프라인 검증 경로(신규)**: `ferryx-cli license verify --file <path> --public-key <key_id:base64>` — 네트워크·서버·데몬 없이
  문서 + 공개키만으로 검증한다. 이 경로가 selfhost의 존재 이유다(§4).
- **키 유출 대응**: 로테이션 + 수용 집합 제거 + 짧은 리스 갱신(§6 D3)을 함께 써야 실효가 있다. 발급된 `selfhost` 문서는
  회수 불가임을 제품 약속으로 문서화한다(§4).

#### 3. 트라이얼 시맨틱 (정밀 정의)

**시계는 "최초 성공 활성화"에서 시작한다.** 가입(`login/consume`, `service.rs:1117`)은 트라이얼을 시작하지 않는다.
`POST /api/account/v1/license/activate`가 성공한 시각을 서버가 `activated_at`으로 기록하고, 그 값으로
`issued_at = not_before = activated_at`, `expires_at = activated_at + 30d`인 문서를 발급한다.
저장은 기존 저장소에 맵 3개를 추가한다(신규, 모두 `#[serde(default)]`라 기존 `account-store.json`이 그대로 로드된다 —
`store.rs:132-147`의 필드별 `#[serde(default)]` 패턴):

- `trials: BTreeMap<trial_key, TrialRecord { subject_id, machine_fingerprint, license_id, activated_at, last_renewed_at }>`
- `licenses: BTreeMap<license_id, LicenseRecord { document_json, subject_id, mode, issued_at, expires_at }>` (감사용, purge 제외)
- `license_revocations: BTreeMap<license_id, RevocationRecord { revoked_at, reason }>` — `purge_expired`(`store.rs:166`)에 정리 추가

**멱등성**: 같은 `trial_key` 재호출은 새 문서를 만들지 않고 `license_id`·`activated_at`이 같은 문서를 남은 기간으로 반환한다.
로컬 파일을 지우고 재설치해도 서버가 정본이므로 트라이얼이 리셋되지 않는다.

##### 3.1 재설치·계정 갈아타기 저항: `trial_key` = 지문 단독

- `machine_fingerprint = sha256(platform_machine_id + FERRYX_FINGERPRINT_SALT)`의 앞 16바이트 hex(신규).
  `platform_machine_id`는 macOS `IOPlatformUUID`, Linux `/etc/machine-id`, Windows `MachineGuid`를 cfg 분기로 읽고
  (규약: 플랫폼 코드는 명시적 모듈 + 폴백, `AGENTS.md:54`), 셋 다 없으면 폴백으로 기존 데몬 Ed25519 기계 신원
  (`enroll_client.rs:10`이 쓰는 `load_or_generate_machine_identity`)의 공개키를 쓴다.
- `trial_key = "trial-" + sha256(machine_fingerprint)`. 계정 단독 키로 하면 같은 기계 + 새 계정으로 30일이 다시 시작된다.
  대가는 공용 장비의 두 번째 사용자가 트라이얼을 못 받는 것 → 운영자 예외 경로
  (`POST /api/account/v1/license/trial-override`, 운영자 토큰 + 사유 기록).
- 폴백 지문(기계 신원 기반)은 재설치 저항이 약하다. 서버는 폴백 지문 활성화에 `require_online_reactivation: true`를 붙여
  7일마다 온라인 재확인을 강제한다.

##### 3.2 시계 조작 저항 (3중)

1. **서버 발급 시각**: 클라이언트는 자기 시각을 보내지 않는다(요청 본문은 `machineId`/`fingerprint`뿐). `not_before`는
   서버가 정한다. 검증기가 `now + 300 < not_before`면 `LICENSE_CLOCK_REWOUND`로 거부한다.
2. **단조 카운터(신규 `LicenseState`)**: `~/.ferryx/remote/license.json`에 `{ document, state }`를 함께 저장하고 매 관측마다
   `state.highest_seen_unix`를 올린다. 유효 시각 `now_effective = max(wall_clock, highest_seen_unix)` — 시계를 뒤로 돌려도
   줄지 않으므로 만료가 연장되지 않는다(AT-2).
3. **서버측 실측**: 릴레이는 attach 시 `MachineRecord.last_seen_at`(`store.rs:84`)을 갱신한다. 온라인 구간은 서버 시각으로
   측정되므로 로컬 시계 조작이 트라이얼/유료 사용량을 속이지 못한다.

##### 3.3 만료 처리 — 유예 → degraded → 제한, 세션은 절대 죽이지 않는다

| 구간 | 조건 | 결과 |
|---|---|---|
| valid | `now_effective <= expires_at` | 전부 허용 |
| degraded | `expires_at < now_effective <= expires_at + grace_seconds` | 경고 배너 + **새** 원격 attach/릴레이 세션만 거부. 로컬 터미널·기존 PTY·worktree 전부 정상 |
| limited | 유예 종료 후 | 새 공식 릴레이 attach 403 `LICENSE_EXPIRED`, 새 grant 발급 403 `LICENSE_REQUIRED`, 로컬 터미널 무기한 유지 |

집행이 "세션 종료"가 아닌 "새 세션 거부"인 이유: 데몬이 모든 PTY master fd를 소유하므로 데몬/세션을 건드리는 순간 진행 중인
에이전트 워크플로가 죽는다(`AGENTS.md:65`, `src-tauri/AGENTS.md:32`). 만료의 실질 압박은 원격·릴레이 기능 상실로 충분하다.

#### 4. selfhost 영구 라이선스

- **획득 경로 2개**: (a) 로그인 후 `POST /api/account/v1/license/selfhost` 1회 호출(온라인, 무료), (b) 운영자 오프라인 발급 —
  `account-signing-key.json`(`store.rs:112`)이 있는 서명 호스트에서 `ferryx-cli license issue --mode selfhost --subject org:<이름>`.
  (b)는 네트워크·계정이 아예 필요 없다.
- **폰홈이 필요 없는 이유**: 검증이 공개키 산술뿐이다(`license_signing_input` + Ed25519 — `account_grants.rs:26`/`:38`이
  릴레이에서 하는 것과 같은 검증). 서버 상태(활성화 기록·폐기 목록)를 참조하지 않으므로 폐쇄망에서도 성립한다.
  대가는 회수 불가 — 발급된 selfhost 문서는 만료도 폐기도 없다. 이것을 버그가 아니라 계약으로 명시한다.
- **공식 릴레이의 구별 (2중)**: ⑴ **키 고정** — 공식 릴레이는 공식 계정 공개키만 핀한다(`bin/relay.rs:44`, 검증 `account_grants.rs:38`).
  selfhost 사용자의 릴레이는 자기 키를 핀하므로 서로의 grant를 받아줄 수 없다. ⑵ **엔타이틀먼트** — selfhost 문서는
  `relay_minutes_per_day = 0`, `features`에 `relay`가 없다(§1.3 두 번째 블록). 게이트는 `mode`가 아니라 엔타이틀먼트 값으로
  판단하므로 selfhost 문서로 공식 릴레이를 쓰려는 시도는 "릴레이 분 0"으로 막힌다. 자기 릴레이는 자기 장비에서 무제한이다.

#### 5. 엔타이틀먼트 모델

JSON 형태(아래는 selfhost 값): `{"max_machines": null, "max_devices": null, "relay_minutes_per_day": 0, "features": ["local", "selfhost_relay"]}`
`null`은 "무제한", `0`은 "불가"다. 구분하지 않으면 selfhost(무제한)와 릴레이 미포함(0)을 한 필드로 표현할 수 없다.

| 키 | trial | paid | selfhost |
|---|---|---|---|
| `max_machines` | 1 | 10 | `null` |
| `max_devices` | 2 | 25 | `null` |
| `relay_minutes_per_day` | 120 | `null` | 0 |
| `features` | `local, relay, remote_attach` | `local, relay, remote_attach, browser, multi_host` | `local, selfhost_relay` |

`grace_seconds` 기본값: trial `259200`(3일), paid `1209600`(14일), selfhost `0`.

**평가**: `LicenseGate::evaluate(doc, now_effective, host) -> Result<Entitlements, LicenseDenial>` (신규 `account/license.rs`).
거부 코드는 계정 API의 구조화 오류 형식(`service.rs:215-219` `ApiError`, `:239-243` `ErrorBody`)을 그대로 쓴다.

```
1 schema    == "ferryx.license.v1"                                        → LICENSE_SCHEMA_UNSUPPORTED
2 signature key_id로 수용 집합에서 공개키 선택 후 검증                     → LICENSE_SIGNATURE_INVALID / LICENSE_KEY_UNKNOWN
3 binding   machine_fingerprint == null || == 현재 지문                    → LICENSE_MACHINE_MISMATCH
4 window    now_effective >= not_before - 300                              → LICENSE_CLOCK_REWOUND
            now_effective <= expires_at (null이면 통과)                    → LICENSE_EXPIRED
5 grace     expires_at < now_effective <= expires_at + grace_seconds       → DEGRADED (차단 아님)
6 normalize machine_limit == entitlements.max_machines && device_limit == entitlements.max_devices &&
            features == entitlements.features                              → LICENSE_INCONSISTENT
```

6단계는 중복 필드를 "검증되는 중복"으로 만든다: 엔진은 최상위 필드만 읽어(핫패스 단순) 서명은 중복까지 덮으므로,
한쪽만 바꾼 문서는 통과하지 못한다(AT-1). **집행 지점**: 릴레이 새 attach(`relay_server.rs:2167` 라우터 계열), grant 발급
(`service.rs:913` `issue_grant`), 게이트웨이 grant 수락(`account_grants.rs:89` `grant_gate_router`). 데몬은 건드리지 않는다(§3.3).

#### 6. 결정

##### D1. 검증: 오프라인 우선 + 주기적 온라인 재확인 (권고 B)
- A) 온라인 전용: 매 기동 서버 확인 → 오프라인/폐쇄망에서 제품이 죽고 selfhost와 정면 충돌.
- B) 오프라인 우선 + 서명 리스: 문서만으로 동작, 만료 7일 전부터 갱신 시도. **← 권고**
- 근거: 서명 검증이 이미 오프라인 가능한 구조다(`account_grants.rs:38`은 핀만으로 검증). 폰홈 실패는 "갱신 지연"이어야 하고
  "차단"이면 안 된다.

##### D2. 저장 위치: `~/.ferryx/remote/license.json` 0600 (권고 B)
- A) OS 키체인: macOS/Windows는 좋지만 헤드리스 Linux(릴레이는 systemd 유닛으로 돈다)에 키체인이 없다.
- B) 파일 0600: `canonical_remote_dir()`(`remote/auth.rs:55` Windows, `:62` HOME → `~/.ferryx/remote`) +
  `write_private_json`(`store.rs:216`, 0600) 재사용. 기존 `account-enrollment.json`과 같은 디렉터리(`enroll_client.rs:17`, `:65`). **← 권고**
- C) B + 키체인 랩: 데스크톱 선택 강화. 1차 범위 제외, `wrapped_by` 필드만 예약.
- 근거: 검증된 개인키 저장 경로(`store.rs:216`) 재사용으로 크로스플랫폼 폴백이 필요 없다.

##### D3. 폐기: 짧은 리스 + 폐기 목록 하이브리드 (권고 C)
- A) 짧은 리스만(30일 슬라이스): 폐기 지연 최대 30일, selfhost에 적용 불가(영구).
- B) 폐기 목록만(`license_revocations`): 즉시성은 좋지만 오프라인 클라이언트에 도달하지 못한다.
- C) 하이브리드: paid/trial은 리스 + 갱신 시 폐기 확인, selfhost는 리스 없음(회수 불가 명시), 악용 신고 시
  `max_machines`/`relay_minutes_per_day`를 하향한 **새 슬라이스만** 발급해 실효 상한을 낮춘다. **← 권고**
- 근거: 서버측 상태는 이미 JSON 저장소에 있고(`store.rs:132-147`) 폐기 맵 추가 비용이 낮다. selfhost는 "영구"가 계약이므로
  사후 회수 장치를 넣는 순간 계약이 깨진다.

##### D4. 집행 지점: 서버측 + GUI 표시, 데몬 무변경 (권고 C)
- A) 데스크톱 GUI만: 우회가 자명하다(CLI·릴레이 직접 호출).
- B) 데몬에서 로컬 기능까지 하드 블록: PTY 소유권 규칙과 충돌한다(`AGENTS.md:65`, `src-tauri/AGENTS.md:32`).
- C) 릴레이 attach·grant 발급에서 원격 기능만 집행 + GUI는 상태 표시. **← 권고**
- 근거: 지불 여부와 무관하게 로컬 터미널은 항상 동작해야 하고, 유료 가치의 대부분은 원격·릴레이 기능에 있다.

##### D5. selfhost 판별: 키 고정 + 엔타이틀먼트 값 (권고 B)
- A) `mode` 문자열 검사만: mode별 특례가 코드에 늘어난다(모드 추가 = 게이트 수정).
- B) 키 고정 + 엔타이틀먼트: 공식 릴레이는 공식 키만 핀(`bin/relay.rs:44`), 게이트는 `relay_minutes_per_day`/`features`로 판단. **← 권고**
- 근거: fail-closed 핀 구조(`account_grants.rs:38-45`)를 재사용하고, 모드 추가에 게이트 수정이 필요 없다.

#### 7. 단계 계획

- **P1 (신규 모듈) `src-tauri/src/account/license.rs`** — `LicenseDocument`, `LicenseMode`, `Entitlements`,
  `LICENSE_DOCUMENT_DOMAIN`, `license_signing_input()`, `verify_license()`, `LicenseGate::evaluate()`,
  `LicenseState { highest_seen_unix }`. `account/mod.rs`에 `pub mod license;` 추가(현재 6개 모듈).
  `machine_fingerprint.rs`(신규, cfg 분기 + 폴백)도 여기서 만든다.
- **P2 (저장소) `src-tauri/src/account/store.rs`** — `AccountStore`(`:132-147`)에 `trials`/`licenses`/`license_revocations`
  추가(`#[serde(default)]`), `purge_expired`(`:166`)에 revocations 정리 추가, `licenses`는 감사용으로 보존.
- **P3 (발급 라우트) `src-tauri/src/account/service.rs`** — `router()`(`:1111`)에 `/api/account/v1/license/activate`·
  `/license/renew`·`/license/selfhost`·`/license/revoke` 추가. 공개키 응답(`:259-263`, 핸들러 `:266`)에 `key_id` 추가.
  발급은 기존 `mutate()`(`:193`, 락 + purge + 원자 저장)를 그대로 쓴다.
- **P4 (집행) `src-tauri/src/remote/license_gate.rs`(신규) + `relay_server.rs:2167` / `account_grants.rs:89`** — 새 attach와
  grant 수락에 게이트 삽입. `FERRYX_LICENSE_ENFORCEMENT`(기본 `off`)로 배포 후 활성화해 기존 사용자 회귀를 0으로 만든다.
- **P5 (클라이언트·CLI) `src-tauri/src/account/license_client.rs`(신규) + `bin/cli.rs`(`Cargo.toml:20-22`)** —
  `ferryx account license status|renew`, `ferryx license verify --file --public-key`. 저장은 `enroll_client.rs:65` 옆 경로에
  `write_private_json`으로(D2).
- **P6 (UI·문서) `ui/src/remote/accountSession.ts`** — `fetchLicense()` 추가(기존 스타일: `resolveAccountOrigin` `:197`,
  `listMachines` `:301`, `requestGrant` `:332`). `ui/src/lib/storageKeys.ts`에 `ferryx.license.state:v1` 추가(`:14`, `:18` 형식,
  `LEGACY_STORAGE_KEY_MAP` `:21` 유지). 일반 설정 탭(`ui/src/lib/generalSettings.ts:70`)에 라이선스 상태 행.
  `docs/account-service.md:12-13`의 낡은 서술 정정 + 신규 `docs/license.md`.

#### 8. 인수 테스트

**AT-1 문서 검증·정규화 (단위)**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib account::license::tests::
```

기대: `license_document_verifies_with_pinned_key`, `unknown_key_id_is_refused`, `tampered_entitlements_break_signature`,
`selfhost_license_has_null_expiry_and_unlimited_caps`, `misnormalized_machine_limit_is_refused` 전부 통과, `0 failed`.
`tampered_*`는 `entitlements.max_machines`만 512로 바꾼 문서가 `LICENSE_SIGNATURE_INVALID` 또는 `LICENSE_INCONSISTENT`로
거부됨을 단언한다.

**AT-2 시계 역행·만료 (단위, 주입 시계)**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib account::license::clock::
```

기대: `rewinding_wall_clock_does_not_extend_trial`(관측 시각을 `expires_at + 1`로 올린 뒤 벽시계를 30일 전으로 되돌려도
`LICENSE_EXPIRED`), `grace_window_reports_degraded_not_expired`, `not_before_in_future_is_clock_rewound` 통과.
테스트는 시각 인자만 받는 `evaluate(doc, now_effective, host)`를 쓴다(수면·타이머 금지).

**AT-3 현재 배포본 공개키 라우트 실측 (curl)**

```bash
curl -sS -o /tmp/pk.json -w '%{http_code}\n' https://relay.checka.cc/api/account/v1/public-key && cat /tmp/pk.json
```

기대: `200` + `{"publicKey":"<base64 32바이트 Ed25519 공개키>"}` — 응답 키 이름은 camelCase다(`service.rs:266` 핸들러 +
`service.rs:259-263` DTO). 로컬은 같은 경로를 로컬 데이터 디렉터리로 띄워 확인한다.

**AT-4 트라이얼 활성화 멱등성 (curl, 로그인 토큰 필요)**

```bash
curl -sS -X POST https://relay.checka.cc/api/account/v1/license/activate \
  -H "Authorization: Bearer $FERRYX_ACCOUNT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"machineId":"<machine_id>","fingerprint":"sha256:<hex16>"}'
```

기대: 1회차 `200` + `{"license":{"mode":"trial","expires_at":<activated_at+2592000>,...},"activated_at":<u64>}`.
로컬 `~/.ferryx/remote/license.json`을 지우고 같은 지문으로 재호출하면 **같은 `license_id`·같은 `activated_at`**이 돌아오고
`expires_at`은 늘지 않는다(트라이얼 리셋 없음).

**AT-5 만료 제한은 새 세션에만 (curl + 데몬 세션 수)**

```bash
curl -sS -o /tmp/attach.json -w '%{http_code}\n' -X POST https://relay.checka.cc/api/v1/attach/session \
  -H "Authorization: Bearer $FERRYX_DEVICE_TOKEN" -H 'Content-Type: application/json' -d '{"machineId":"<machine_id>"}'
cat /tmp/attach.json
```

기대: 만료(유예 종료) 상태에서 `403` + `{"code":"LICENSE_EXPIRED",...}`, 그리고 같은 시각에 데스크톱의 기존 터미널 세션은
그대로 살아 있어야 한다(세션 수 불변, 프로세스 종료 0) — 규칙 출처 `AGENTS.md:65`, `src-tauri/AGENTS.md:32`.

**AT-6 서명 게이트 회귀 없음 (기존 + 신규 1건)**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib remote::account_grants
```

기대: 기존 서명 게이트 테스트(핀 없음/서명 불일치 → 403, `account_grants.rs:38-45`) 전부 통과 +
`selfhost_license_is_not_entitled_to_official_relay_minutes` 통과, `0 failed`.

#### 9. 리스크·열린 질문

- **소스 라이선스**: `Cargo.toml:6`은 `license = "SUL-1.0"`이다. 상용 모델(체험/구독/셀프호스트)과 소스 라이선스를 함께
  고지하는 문구가 필요하다 — 이 섹션 범위 밖, 배포 섹션에서 다룬다.
- **지문 프라이버시**: `IOPlatformUUID`/`/etc/machine-id`/`MachineGuid` 원문은 전송하지 않는다. 솔트 해시 앞 16바이트만 보낸다.
- **미결**: `relay_minutes_per_day` 계량 지점(릴레이 세션 수명 합산)은 릴레이에 카운터가 없다 — P4에서 `RelayState`
  (`relay_server.rs:276`)에 일별 카운터를 둘지 별도 계량 저장소를 둘지 릴레이 섹션에서 결정한다.

</details>

### 3.2 30일 무료체험 메커니즘
- 시작 트리거, 만료 판정, 유예, 재설치 리셋 방지, 시계 조작 내성 → 상세: `01-ls-core.md` + 알림 흐름 `04-email-account-flows.md`
- 핵심 계약(요약): 체험은 **서버가 판정**한다(클라이언트 시계 불신). **시작 시점은 단 하나로 정의한다: 첫 성공한 `/machines/enroll` 시각**(가입 시점도, 모호한 "최초 활성화"도 아니다 — sol-6 B5 수용). 서버가 `trial_started_at`을 기록하고, 클라이언트는 짧은 만료의 라이선스로 오프라인 동작하되 유예 후 재확인한다. 재설치 저항은 **계정과 무관한 서버측 키드 지문 이력**(`trial_fingerprints: HMAC(server_key, machine_fingerprint) → first_seen_at`)으로 구현한다(sol-6 신규B 수용 — 계정 ID를 섞으면 정책이 성립하지 않는다). 180일 창 내 재등장 시 신규 계정이라도 체험을 거부하며, 이 이력은 **개인정보 고지·보존기간·삭제 요청·오탐 이의제기** 절차에 묶인다(실행 계약: §10 **B9b**).

### 3.3 셀프호스팅 영구 무료 라이선스
- 획득 경로·영구성·폰홈 없음·공식 relay와의 구분 → 상세: `01-ls-core.md`, 운영 `06-ops-data-security.md`
- 핵심 계약(요약): `mode=selfhost`의 **무만료·무기기제한** 라이선스이며, **`FERRYX_DEPLOYMENT_MODE=selfhost`로 기동한** relay만 **라이선스 검사 자체를 수행하지 않는다**(모드 미설정 기동은 거부 — fail-open 금지).

### 3.4 Entitlement / 쿼터 모델
- 필드·기본값·평가 지점 → 상세: `01-ls-core.md`(모델) + `02-backend-enforcement.md`(강제)
- 핵심 계약(요약): `entitlements = { max_machines, max_devices, relay_minutes_per_day, features[] }`. **각 항목의 계약(sol-6 B9 수용)**: *단위*(대/분), *리셋*(UTC 자정), *카운터 소유자*(relay 데이터스토어), *원자적 예약/회계*(세션 시작 시 선예약, 종료 시 정산), *동시 세션*(합산 소비), *실패 복구*(재기동 후 카운터 보존), *초과*(세션 종료가 아니라 신규 연결 거부). 평가 지점은 기기 등록·attach·프록시다.

### 3.5 백엔드(relay + 계정 서비스) 변경
- 강제 지점 5곳, 소유 관계 바인딩, 오류 코드, 레이트리밋 → 상세: `02-backend-enforcement.md`
- **기기 등록 프로토콜은 2개로 분리한다(sol-6 B2 수용)**: (a) **commercial** = 계정 세션 + 기기 소유 증명(기존 attach 키) + 소유 바인딩 + entitlement 검사, (b) **selfhost** = **운영자가 발급한 등록 시크릿을 계속 사용**(현행 `FERRYX_RELAY_MACHINE_TOKENS` 경로를 폐기가 아니라 **selfhost 전용으로 존속·문서화**). 두 경로 모두 등록·페어링·터널·재접속 수용 테스트를 갖는다.
- 핵심 계약(요약): 기기 등록이 **정적 공유 비밀**(현행 `relay_server.rs:286,348`)에서 **계정 소유 + entitlement 검사**로 이동한다. 오류 코드: `TRIAL_EXPIRED`, `LICENSE_INVALID`, `ENTITLEMENT_EXCEEDED`, `OWNERSHIP_MISMATCH`.


<details>
<summary>레인 상세 원문 (02-backend-enforcement.md)</summary>

# 02. 백엔드 라이선스 강제 (Backend Enforcement)

> 범위: 공식 relay(`ferryx-relay`)가 계정 서비스를 내장한 배포에서 **어느 함수가, 무엇을 검사하고, 어떤 코드로 거절하는지**의 설계.
> 불변식: **self-host relay는 account service 없이 오늘과 동일하게 동작해야 한다.** 라이선스 검사는 self-host 경로에 절대 걸리면 안 된다.

#### 0. 현재 상태 (코드로 확인된 사실)

- `license`라는 Rust 식별자는 `src-tauri/src` 전체에서 0건이다(AST 식별자 검색). 즉 라이선스 계층은 100% 신규이며, 기존 소유권/서명 자산 위에 얹는다. 저장소 메타데이터에는 이미 `license = "SUL-1.0"`이 선언되어 있다(src-tauri/Cargo.toml:6).
- 재사용할 기존 자산:
  - 소유권: `MachineRecord.owner_user_id`(src-tauri/src/account/store.rs:71-87), 조회 `machine_for_owner`(src-tauri/src/account/service.rs:357-371), 목록 필터 `list_machines`(src-tauri/src/account/service.rs:710-725), 발급 시 소유권 확인 `issue_grant`(src-tauri/src/account/service.rs:913-933).
  - 인증: 계정 bearer(`require_user` account/service.rs:338-348 → `authenticate_user` :350-355), 데몬 Ed25519 challenge(`authenticate_control_socket` remote/relay_server.rs:2394-2451), 서명된 grant 제출 검증(`verify_grant_signature` remote/account_grants.rs:38-68).
  - machine↔public key 고정: `bind_machine_key`(remote/relay_server.rs:547-630) + 영속 `machine_keys.json`(로드·검증 `with_key_store` remote/relay_server.rs:407-421, 게시 `bind_machine_key` :547-630).
  - 정적 공유 비밀: `FERRYX_RELAY_MACHINE_TOKENS`(bin/relay.rs:34) → `validate_machine_token`(remote/relay_server.rs:678-688).
- 아직 없는 것: (i) relay 안의 machine→user 매핑(control channel 키가 machine_id 문자열뿐: `register_control_channel` remote/relay_server.rs:691-710), (ii) 라이선스/시험판 상태 저장, (iii) `/host/{machine}` 프록시의 소유권·자격 검사.

#### 1. 강제 지점 (enforcement points)

| # | 지점 | 진입 함수 (file:line) | 오늘 검사 | 추가할 검사 |
|---|---|---|---|---|
| a | machine 등록 / control channel | `control_handler` remote/relay_server.rs:2348 → `authenticate_control_socket` :2394 → `validate_machine_token` :678 → `bind_machine_key` :547 | 정적 bearer 화이트리스트(:2371), 예약 machine_id 거부(:2423), challenge+audience 서명(:2394-2451), machine↔key 최초 고정(:547-630), private relay의 `enrollment_token` 재검사(:556-563) | 계정에 바인딩된 enrollment 증명(§1.1), 라이선스 모드·만료, seats 상한 |
| b | `POST /api/v1/pair/exchange` (라우트 :2182, 호스트 프록시 경유 :1673-1690) | `pair_exchange_handler` :1556 → `exchange_http` :1864 → `claim_pairing` :1000 | PIN/`pairing_token` 매칭과 lease 상한(`register_pairing` :899), relay가 stamp한 control generation liveness(:1047-1056), claim fence/만료(`consume_pairing` :968, `rollback_pairing` :982), per-IP pairing admission과 5회 실패 lockout(:1000 진입부, 실패 누적은 같은 함수 후반부) | 등록 machine의 엔타이틀먼트(license mode/expiry), machine당 pair 분당 상한 |
| c | `POST /api/v1/attach/session` (라우트 :2183) | `attach_session_handler` :1578 | 429 `RATE_LIMITED`(:1586-1590), account service 없으면 404(:1595-1599), bearer 세션(:1603), **소유권** `machine_for_owner`(:1620) → 404 `MACHINE_NOT_FOUND`(:1628), control channel 생존 → 409 `MACHINE_OFFLINE`(:1641/:1649/:1659) | 엔타이틀먼트 활성 여부(`TRIAL_EXPIRED`/`LICENSE_INVALID`), seats 초과(`ENTITLEMENT_EXCEEDED`) |
| d | `/host/{machine_id}/api/v1/{*path}`(라우트 :2216) + `/tunnel/control`(:2217) | `host_http_handler` :1673 → `allowed_http_route` :1763 / `proxy_http` :1936, `control_handler` :2348 | 경로 화이트리스트(:1763-1794), 쿼리 검증(`validate_http_query` :1819-1916), 원샷 소켓 티켓(`socket_ticket_handler` :1158, `consume_socket_ticket` :1280), 데몬 device token 재검증(`authorize_device_token` :512-545) | machine→owner 조회 후 자격 확인(`OWNERSHIP_MISMATCH`), 만료 시 신규 프록시 거부 |
| e | `POST /api/account/v1/machines/enroll` (라우터 account/service.rs:1111-1139, 라우트 :1131) | `enroll` account/service.rs:782, `enroll_challenge` :767, `create_enrollment_code` :689 | 1회용 challenge(`open_challenge` :160-179 / `take_challenge` :181-188), Ed25519 enrollment 서명(:807-821, `ENROLL_SIGNATURE_INVALID` :818), 코드 만료·origin 검증, machine 탈취 시 409 `ACCOUNT_MACHINE_CLAIMED`(:857), epoch 증가 | 라이선스 발급/상태 기록, seats 초과 시 409 `ENTITLEMENT_EXCEEDED`, trial 시작 시각 |

##### 1.1 (a) machine 등록: 정적 토큰을 계정 증명으로 대체

오늘 이 경로는 두 갈래다. ① 레거시 bearer 토큰은 `control_handler`(relay_server.rs:2348)가 `validate_machine_token`(:678)으로 화이트리스트 비교만 하고 통과시킨다(:2371). ② Ed25519 신원은 `authenticate_control_socket`(:2394)에서 challenge 서명을 검증하고 `bind_machine_key`(:547)로 machine_id↔공개키를 **최초 1회 고정**한다. 다만 relay가 machine 토큰을 하나라도 설정한 "private relay"라면 enrollment 자체에 `enrollment_token`(=같은 정적 토큰)이 다시 필요하다(:556-563, `"Enrollment token required for private relay"`).

설계: ②의 `bind_machine_key` 호출 **직전**에 계정 검증 훅을 추가한다. `ControlAuth`가 이미 `machine_id`·`public_key`를 싣고 있으므로(:2394-2451), relay는 `AccountState`에서 `machine_id`로 `MachineRecord`를 찾아 (i) 존재 여부, (ii) `public_key` 일치(≠이면 `OWNERSHIP_MISMATCH`), (iii) `owner_user_id`의 엔타이틀먼트를 확인한다. 레거시 bearer 경로(:2371)는 **self-host 호환을 위해 그대로 둔다**.

##### 1.2 (b) pair/exchange: 기기 페어링은 machine 자격에 종속

`exchange_http`(:1864)는 PIN을 `claim_pairing`(:1000)으로 점유한 뒤 데몬 게이트웨이에 `/api/v1/pair/exchange`를 프록시하고, 성공 시에만 `claim.commit()`(:1911)으로 페어링을 소비한다. 이 지점에는 계정 개념이 전혀 없다. 추가 검사는 `claim_pairing`의 성공 반환 **직전**(:1072-1075 주석 직후)에 두고, 그 machine의 owner 엔타이틀먼트가 비활성이면 403 `TRIAL_EXPIRED`/`LICENSE_INVALID`로 거절한다. 이유: 페어링은 device token을 발급하는 입구이므로(`claim_pairing`의 자기-PIN 등록 한계 주석 :1072-1075를 고려) 여기서 막지 않으면 (c)만으로는 자격 만료가 새 기기 확산을 막지 못한다.

##### 1.3 (c) attach/session: 이미 있는 소유권 게이트에 엔타이틀먼트를 붙인다

`attach_session_handler`(:1578)는 이미 ① IP admission 429(:1586-1590), ② account service 부재 시 404 `"relay is not an account origin"`(:1595-1599), ③ bearer 세션 검증(:1603), ④ **소유권** `machine_for_owner`(:1620) 실패 시 404 `MACHINE_NOT_FOUND`(:1628), ⑤ machine 오프라인 409(:1641-1660)를 수행한다. 추가는 ④와 ⑤ 사이에 `entitlement_gate(state, &user, &machine)` 한 번이다. 응답은 새 코드(§4)로 나간다.

##### 1.4 (d) host 프록시·control 채널: 오늘 소유권 검사가 없는 유일한 구멍

`host_http_handler`(:1673)는 `allowed_http_route`(:1763)와 쿼리 검증(:1819)만 통과하면 `proxy_http`(:1936)로 임의 machine에 요청을 흘려보낸다. machine_id를 아는 공격자/만료 계정이 소유하지 않은 machine의 HTTP 표면에 도달할 수 있다는 뜻이다(데몬 게이트웨이의 자체 인증이 마지막 방어선). 여기에 `machine_owner(machine_id)` 조회를 추가하고, 요청에 실린 계정 세션이 없거나 소유자가 아니면 404 `OWNERSHIP_MISMATCH`로 거절한다. `/tunnel/control`(:2217)은 (a)의 게이트를 그대로 상속한다.

##### 1.5 (e) machines/enroll: 라이선스 발급의 단일 지점

`enroll`(account/service.rs:782)은 challenge(:780), 서명(:807-821), 코드 소유권·만료·origin(:812-834), machine 탈취 409(:857), epoch 증가까지 이미 견고하다. 라이선스 상태(모드·만료·seats)를 **여기서만** 생성/갱신하는 것이 옳다. 다른 엔드포인트가 라이선스를 만들면 상태가 두 곳에 생긴다.

#### 2. 소유권 바인딩 (ownership binding)

- **이미 있다.** `machines[].owner_user_id`(account/store.rs:71-87)와 `machine_for_owner`(account/service.rs:357-371), `issue_grant`의 소유권 비교(account/service.rs:913-933, 불일치 응답 :933)가 그대로 "사용자 U가 machine M을 소유하는가"에 답한다. 새 소유권 테이블은 만들지 않는다(중복 진실 금지).
- **추가할 것**: (i) `AccountStore`에 `entitlements: BTreeMap<String, EntitlementRecord>` (key = `user_id`) — `mode`(`Trial|Paid|Grandfathered`), `seats`, `expires_at`, `trial_started_at`, `status`. 기존 맵 전부가 `#[serde(default)]`이므로(account/store.rs:132-148) 신규 필드도 `#[serde(default)]`로 넣어 구버전 JSON이 그대로 로드되게 한다. (ii) relay 쪽 캐시: `ControlChannel`에 `owner_user_id`와 `entitlement_snapshot`을 넣어(remote/relay_server.rs:691-710의 등록 시점에 채움) 요청마다 `AccountState::read`(매 호출 JSON 로드, account/service.rs:206-210)로 파일을 다시 읽지 않게 한다.
- **기존 machine 마이그레이션**: 운영 relay의 account store에는 현재 3대가 등록되어 있다(사용자 제공 전제 — 이 문서 작성 시점에 직접 조회로 확인하지는 않았다). 이 3대의 `owner_user_id`는 이미 채워져 있으므로 **machine 레코드는 손대지 않는다**. 대신 부팅 시 1회성 마이그레이션 `AccountStore::migrate_legacy_entitlements()`를 추가해, 엔타이틀먼트가 없는 `owner_user_id`마다 `mode = Grandfathered`, `expires_at = u64::MAX`, `seats = 1` 레코드를 생성하고 `save`(account/store.rs:161)로 기록한다. 이 단계를 빼면 배포 즉시 기존 3대가 "라이선스 없음" 상태가 되어 운영이 끊긴다 — 마이그레이션은 선택이 아니라 배포 전제다. 실행 지점은 두 진입점 모두: `ferryx-relay` 기동(bin/relay.rs:149-153)과 `ferryx-account`(src-tauri/src/bin/account.rs:34; 바이너리명은 Cargo.toml의 `[[bin]] name = "ferryx-account"`).

##### 2.1 store 변경 스케치 (구현 시 그대로 사용)

```rust
// src-tauri/src/account/store.rs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementRecord {
    pub owner_user_id: String,
    pub mode: String,          // "trial" | "paid" | "grandfathered"
    pub seats: u32,
    pub trial_started_at: u64,
    pub expires_at: u64,       // grandfathered = u64::MAX
    pub status: String,        // "active" | "suspended" | "revoked"
}
```

게이트 판정은 한 함수에 모은다(§1의 네 지점이 서로 다른 규칙으로 갈라지는 것을 막는다):

```rust
// src-tauri/src/remote/relay_server.rs (신규)
pub(crate) enum LicenseGate { Allowed, TrialExpired, Invalid, Seats, ForeignOwner }
fn license_gate(state: &RelayState, user_id: &str, machine_id: &str) -> LicenseGate
```

각 호출부는 `LicenseGate` → §4 오류 코드 매핑만 담당한다. machine 소유권은 `machine_for_owner`(account/service.rs:357-371)를 그대로 쓰고, relay는 `AccountState`를 `account_state()`(remote/relay_server.rs:475)로 이미 들고 있다.

#### 3. trial / paid / self-host 서버측 차이

| 모드 | account 라우터 | 강제되는 것 | 근거(오늘 코드) |
|---|---|---|---|
| 공식 relay (Trial) | 병합됨 | machine 등록·pair·attach·host 프록시에 엔타이틀먼트 필요. 만료 시 신규 attach/pair 거부 | `relay_router_with_account`(remote/relay_server.rs:2167)가 `account_state`가 있을 때만 라우터를 merge(:2233) |
| 공식 relay (Paid) | 병합됨 | 위 + seats/기기 수 상한, 초과 시 409 | 동일 + `enroll`(account/service.rs:782)의 seats 검사(신규) |
| self-host relay | 병합 안 됨 | **아무것도 강제하지 않음.** 정적 machine token + Ed25519 신원만 | `relay_router`(remote/relay_server.rs:2160) → `relay_router_with_grant_key(state, None)` → `account_state == None` → attach/session은 404(:1595-1599), grant 제출은 무조건 403(account_grants.rs:42-44: pinning된 키가 없으면 `FORBIDDEN`) |

self-host 경로 보존이 이 설계의 1번 제약이다. 근거: (i) 계정 기능은 origin이 설정된 경우에만 켜진다 — `account_origin()`은 `FERRYX_ACCOUNT_ORIGIN`도 파일도 없으면 `ACCOUNT_ORIGIN_UNSET` 오류를 반환한다(src-tauri/src/account/origin.rs:16-36). (ii) relay 바이너리는 `--account-public-key`/`FERRYX_RELAY_ACCOUNT_PUBLIC_KEY`가 없어도 기동한다(bin/relay.rs의 `parse_args`, 호출부 :153). (iii) 데몬 쪽 계정 참조는 로컬 enrollment 레코드 읽기(`load_enrollment_record()` remote/attach_router.rs:51·:202, remote/relay_client.rs:680)와 CLI 계정 명령(`ferryx account enroll|login`, src-tauri/src/cli.rs:721, `enroll_machine` :784, device-auth 경유 :845)뿐이고, 라이선스 서버 호출은 없다. 따라서 "공식 = 강제, self-host = 무강제"는 새 플래그가 아니라 **account_state 유무**로 이미 표현된다. 새 검사는 전부 `if let Some(account) = state.account_state()` 안쪽에 둔다.

#### 4. 오류 계약 (error contract)

오늘의 구조화 오류는 `ApiError { status, code: &'static str, message }`(account/service.rs:217-235) → `ErrorBody { code, message }`(구조체 account/service.rs:242, camelCase 직렬화, `into_response` account/service.rs:247-253)이다. 실예: `RATE_LIMITED`(relay_server.rs:1588, 429), `MACHINE_NOT_FOUND`(:1628, 404), `MACHINE_OFFLINE`(:1641, 409), `ACCOUNT_MACHINE_CLAIMED`(account/service.rs:857, 409), `ENROLL_SIGNATURE_INVALID`(account/service.rs:818, 401). 주의: `details` 필드는 현재 존재하지 않는다(AGENTS.md의 `{code, message, details}` 서술과 실제 구현이 다름). 프론트가 `details`를 파싱하려면 `ErrorBody`에 `#[serde(skip_serializing_if = "Option::is_none")] details: Option<serde_json::Value>`를 additive로 추가해야 한다(기존 응답 바이트는 그대로).

| 신규 코드 | HTTP | 발생 지점 | `details`(추가 시) | 클라이언트 기대 동작 |
|---|---|---|---|---|
| `TRIAL_EXPIRED` | 403 | §1.2·§1.3 게이트 | `{ "trialEndedAt": <u64> }` | 업그레이드 안내, 재시도 금지 |
| `LICENSE_INVALID` | 403 | §1.2·§1.3 게이트(서명·상태 불일치) | `{ "reason": "signature|revoked|mode" }` | 재로그인 후 1회 재시도 |
| `ENTITLEMENT_EXCEEDED` | 409 | §1.5 `enroll`, §1.3 게이트(seats) | `{ "seats": <n>, "used": <n> }` | 기기 해제 또는 상향 결제 |
| `OWNERSHIP_MISMATCH` | 404 | §1.1 `bind`, §1.4 host 프록시 | — | 토큰 폐기 |

거절 응답 예(신규):

```json
{"code":"TRIAL_EXPIRED","message":"trial ended","details":{"trialEndedAt":1790000000}}
```

오늘의 `RATE_LIMITED`(:1588)나 `MACHINE_NOT_FOUND`(:1628) 응답은 `code`/`message` 두 필드뿐이므로, `details`는 additive로만 추가하고 프론트는 부재를 정상 경로로 처리해야 한다.

`OWNERSHIP_MISMATCH`를 403이 아니라 404로 두는 이유: 오늘 `attach_session_handler`가 타 계정 machine에 대해 이미 404 `MACHINE_NOT_FOUND`(:1628)를 반환하며 존재 여부를 누출하지 않는다. 같은 규칙을 유지한다.

#### 5. 속도 제한 / 남용 방지

오늘 있는 것:
- relay 채널 admission: `admit()` — 60초 창(`ADMISSION_WINDOW` remote/relay_server.rs:320)당 30회, 실패 5회 → 60초 lockout(`record_auth` :665-677). 모든 강제 지점에서 호출된다(:1586, :2348, :2590, :2604, :2620).
- 페어링 추측 방어: `claim_pairing`(:1000)의 별도 admission + 실패 5회 lockout + `register_pairing`(:899)의 lease 상한(`MAX_PAIRING_LEASE`)과 machine당 활성 PIN 상한.
- 계정 로그인: `allow_login_request`(account/service.rs:149-158) — 이메일 키, 기본 5회/시간(`DEFAULT_LOGIN_REQUESTS_PER_HOUR` account/store.rs:13). 본문 상한 4096바이트(`DEFAULT_MAX_BODY_BYTES` account/store.rs:14, 라우터 `DefaultBodyLimit` :1138).
- machine 수 상한: 없음. signup 상한도 IP 기준이 없음.

추가할 것:
1. **per-account machine 생성**: `enroll`(account/service.rs:782)에서 `owner_user_id`의 machine 수 ≥ `entitlements.seats`면 409 `ENTITLEMENT_EXCEEDED`.
2. **per-IP signup**: `login_consume`(:628)와 `device_request`(:415)에 IP 키 제한을 추가한다(`allow_login_request`는 email 키라 이메일 로테이션에 무방비). 기존 `AttemptTracker`(:326-331) 패턴을 재사용.
3. **relay 분당 할당 쿼터**: `issue_session`/`open_session_channel`(remote/relay_server.rs:1096-1157)에 owner별 토큰버킷을 두고, 초과 시 429. `MAX_PENDING_SESSIONS` 상한(`open_session_channel` :1096-1157)은 전역이라 계정 격리가 안 된다.
4. **pair 등록 남용**: 자기 PIN을 스스로 등록할 수 있다는 사실은 `claim_pairing` 주석(:1072-1075)이 이미 명시한다. 라이선스 검사(§1.2)를 이 사실에 의존시키지 말고, 등록 자체에 machine당 분당 상한을 둔다.

| 쿼터 | 키 | 창 | 한도 | 위치 |
|---|---|---|---|---|
| 채널 admission | IP | 60초 | 30회 | 기존 `admit`(remote/relay_server.rs:637-660) |
| 페어링 추측 | IP | 60초 | 실패 5회 → lockout | 기존 `claim_pairing`(:1000) |
| 로그인 메일 | email | 1시간 | 5회 | 기존 `allow_login_request`(account/service.rs:149-158) |
| machine 생성 | user_id | 누적 | `entitlements.seats` | 신규 `enroll`(account/service.rs:782) |
| signup | IP | 1시간 | 10회(제안) | 신규 `device_request`(:415) |
| 세션 할당 | user_id | 60초 | 60회(제안) | 신규 `open_session_channel`(remote/relay_server.rs:1096) |

#### 6. 결정 (decisions)

##### D1. 강제 위치: relay 전용 (권고) vs relay + daemon 이중 강제

- **권고: relay 전용.** 데몬은 계정 서비스에 대한 어떤 라이선스 의존도 이미 갖고 있지 않고(`license` 식별자 0건, 계정 참조는 로컬 enrollment/CLI뿐), 소유권 판정의 진실 원천은 account store다(account/service.rs:357-371). 이중 강제는 (i) self-host 경로를 깰 위험(account_state 부재가 데몬에서는 정상)과 (ii) 검증 표면 2배를 만든다.
- 데몬이 정책을 받아야 하는 지점이 생기면 다음 단계로 "서명된 policy claim"만 전달한다: `issue_grant`가 이미 offer를 `attach_public_key`로 봉인해 relay→데몬으로 배달한다(account/service.rs:913-960, `deliver_grant_offer` remote/relay_server.rs:727-755). entitlement claim도 같은 봉인 경로에 실으면 데몬은 서명 검증만 하면 된다. 이번 단계에서는 구현하지 않는다.

##### D2. 라이선스 없는 상태에서 데몬이 기동을 거부해야 하는가? — **아니오** (self-host 보존)

- 옵션 A: 데몬이 로컬 라이선스를 검사하고 없으면 기동 거부. 옵션 B(권고): 데몬은 항상 기동하고, 강제는 relay 경계에서만.
- 근거: (i) self-host 설치는 relay와 데몬을 계정 서비스 없이 돌리는 것이 정상 구성이다 — `account_origin()`이 설정 없으면 오류를 반환한다(account/origin.rs:16-36). (ii) 데몬은 모든 PTY의 소유자다(src-tauri/AGENTS.md 안티패턴: 데몬 강제 종료 금지). 기동 거부는 진행 중 에이전트 작업을 죽이는 최악의 실패 모드다. (iii) 로컬 검사는 우회 가능하므로 보안 이득이 작다. → 데몬은 라이선스 상태를 **표시만** 한다(soft warning).

##### D3. 만료 시 차단 범위: 신규 attach/pair만 (권고)

기존 라이브 세션을 끊지 않는다. 만료 판정은 §1.2·§1.3의 진입 시점에만 적용하고, 이미 발급된 세션(`issue_session`)과 열린 웹소켓은 유지한다. "세션 유실 금지" 원칙과 정합적이며, relay는 이미 세션을 예약만 하고 파괴하지 않는 구조다(`reserve_half` remote/relay_server.rs:814-857).

##### D4. 라이선스 변경 전파: TTL 캐시 + 명시 무효화 (권고)

`ControlChannel`에 엔타이틀먼트 스냅샷을 캐시하면(§2) 만료·좌석 변경이 즉시 반영되지 않는다.

- 옵션 A: 캐시 없이 매 요청마다 store를 읽는다 — 항상 정확하지만 `AccountState::read`가 매번 JSON을 파싱한다(account/service.rs:206-210).
- 옵션 B(권고): 30초 TTL 캐시 + 결제·해제 시 `RelayState::invalidate_entitlement(machine_id)` 명시 무효화.
- 근거: 같은 모양의 TTL 캐시가 이미 있다(device token 30초 — `DEVICE_TOKEN_CACHE_TTL` remote/relay_server.rs:322, `has_fresh_device_token` :500-510). 그리고 D3에 따라 만료되어도 라이브 세션을 끊지 않으므로 초 단위 정밀도가 필요 없다.

#### 7. 단계 계획과 수용 테스트

##### Phase 1 — store + 마이그레이션 (선행 조건)
경로: `src-tauri/src/account/store.rs`(`EntitlementRecord`, `AccountStore::entitlements`, `migrate_legacy_entitlements`), `src-tauri/src/account/service.rs`(`AccountState::entitlement_for_user`), `src-tauri/src/bin/relay.rs:149`·`src-tauri/src/bin/account.rs:34`(기동 시 1회 호출).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib account::store` → 기존 `account-store.json`(entitlements 필드 없음)을 로드해도 오류 0, 저장 후 재로드 시 라운드트립 동일.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::legacy_machine_receives_grandfathered_entitlement` → `owner_user_id` 1명 + machine 3대 픽스처에서 `migrate_legacy_entitlements()` 후 `entitlements` 길이 1, `mode = Grandfathered`, 세 machine의 attach 판정이 모두 허용.

##### Phase 2 — relay 강제
경로: `src-tauri/src/remote/relay_server.rs`(`attach_session_handler` :1578, `exchange_http` :1864/`claim_pairing` :1000, `host_http_handler` :1673, `authenticate_control_socket` :2394) + `src-tauri/src/remote/account_grants.rs`(§4 코드 상수).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::attach_session_refuses_an_expired_trial` → 403 + body `{"code":"TRIAL_EXPIRED","message":...}`. 기존 픽스처 `test_account_state`(relay_server.rs:6545)의 `owned_machine_id`/`other_machine_id` 재사용.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::host_proxy_refuses_a_foreign_owner` → 404 + `{"code":"OWNERSHIP_MISMATCH"}` (오늘 이 경로는 200을 반환하므로 RED가 먼저 관측되어야 한다).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::self_host_relay_without_account_state_keeps_serving_machine_tokens` → `relay_router`(:2160) + `test-machine-token`으로 `/api/v1/attach/session`이 404(계정 아님)이고 정적 토큰 control 채널은 유지됨. **self-host 회귀 방지선.**
- 수동 실측:
  - `cargo run --manifest-path src-tauri/Cargo.toml --bin ferryx-relay -- --port 8787 --machine-token test-token`
  - `curl -sS -o /tmp/attach.json -w '%{http_code}\n' -X POST http://127.0.0.1:8787/api/v1/attach/session -H "Authorization: Bearer $SESSION" -H 'content-type: application/json' -d '{"machineId":"m-1"}'` → 정상 200 `{"sessionId":...,"machineId":"m-1","opaque":true}`, 만료 계정 403 `TRIAL_EXPIRED`.
  - `curl -sS -X POST http://127.0.0.1:8787/api/account/v1/machines/enroll/challenge -H 'content-type: application/json' -d '{"machineId":"m-4"}'` → 200 `{"nonce":...,"timestamp":...,"audience":...,"expiresAt":...}`(account_protocol.rs:13-19 필드명).

##### Phase 3 — 오류 계약 + 쿼터
경로: `ErrorBody`(구조체 :242, `into_response` :247-253) `details` 추가, `enroll`(:782) seats, `allow_*` IP 키, `open_session_channel`(:1096) 쿼터.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::enroll_beyond_seats_reports_entitlement_exceeded` → 409 + `{"code":"ENTITLEMENT_EXCEEDED","details":{"seats":1,"used":1}}`.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::self_host_grant_submission_stays_forbidden_without_pinned_key` → 무서명 relay에서 403(account_grants.rs:42-44 동작 고정).
- 기존 회귀: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server`, `--test account_enroll_flow`, `--test account_login_flow`.

#### 8. 리스크 / 미검증

- 운영 store의 "machine 3대"는 사용자 제공 전제이며 이 문서에서 직접 조회하지 않았다. Phase 1 수용 테스트가 그 전제를 픽스처로 고정한다.
- `ControlChannel`에 owner 캐시를 넣으면 `unregister_control_channel`(:710) 주기와 어긋난 stale 캐시가 생길 수 있다. 라이선스 상태 변경(seats/만료) 후 캐시 무효화 경로를 Phase 2에서 반드시 명시할 것.
- `load_enrollment_record()`(remote/attach_router.rs:51·:202, remote/relay_client.rs:680)의 Option 처리와 데몬의 self-host 동작은 이 문서에서 라인 단위로 확정하지 않았다. D2를 뒤집는 근거로 쓰려면 별도 확인이 필요하다.

</details>

### 3.6 데이터 스토어 & 마이그레이션
- JSON → SQLite, 스키마, 백업/복원, 무중단 전환 → 상세: `06-ops-data-security.md`
- 핵심 계약(요약): 현행 `AccountStore`(JSON + 파일락, `store.rs:132,150,161,195`)에 스키마 버전을 도입하고, 결제 이벤트 도입 시점에 SQLite로 승격한다. 원본 JSON은 보존(되돌림 가능).

### 3.7 결제 / MoR 통합
- MoR 선택, 웹훅, 구독 상태 머신, 세금/VAT, 환불, 다우닝 → 상세: `03-payment-mor.md`
- 핵심 계약(요약): 결제 상태는 **웹훅이 유일한 진실 원천**이며, 이벤트 ID로 멱등 처리한다. 웹훅 서명 검증은 기존 서명 검증 패턴을 따른다.


<details>
<summary>레인 상세 원문 (03-payment-mor.md)</summary>

# 03. 결제 · Merchant of Record 통합 설계

범위: Ferryx 상용화(릴레이 라이선스) 계획 중 **결제 수단 · 세금 대행 · 구독 상태 관리**. 기준: 2026-09-25 `main` 워킹트리.
수수료·지원국 수치는 같은 날짜 웹 검색 기준이며 계약 시점에 재확인한다(§2.1 출처).

## 1. 현재 상태 — 결제 코드는 0건이다

### 1.1 확인한 파일 (전수)

| 파일 | 확인 범위 | 결론 |
|---|---|---|
| `src-tauri/src/account/mod.rs` | 전문(1-6행) | 모듈 6개(`enroll_client`·`mailer`·`offer_sink`·`origin`·`service`·`store`), billing 없음 |
| `src-tauri/src/account/store.rs` | 전문(1-301행) | `AccountStore` 필드 7개(133-148), 만료 정리(166-172), flock(195-213), 원자적 JSON 쓰기(216-236) |
| `src-tauri/src/account/service.rs` | 라우터 1111-1145행 + 상태·헬퍼 1-330행 | 라우트 13개(1113-1137), 결제 라우트 없음 |
| `src-tauri/src/account/{origin,mailer,offer_sink,enroll_client}.rs`, `src-tauri/src/bin/account.rs` | ast-grep 전수 검색 + 전문(1-47행) | 결제 심볼 0건, 계정 서비스 기동(46-48)·기본 바인드 `127.0.0.1:43822`(24행) |
| `src-tauri/src/remote/account_grants.rs` | 전문 | 서명 검증(14·26·38행), GrantGate 라우터(89행) |
| `src-tauri/src/remote/{relay_server,server}.rs` | 모듈 문서(relay 1-30행) + 라우터(2181·4018행 이하) | 터널·호스트 프록시·게이트웨이 라우트만, 결제 라우트 없음 |
| `src-tauri/Cargo.toml` | 전문(1-205행) | 결제 SDK 0건, `sha2` 있음(store.rs:8)·`hmac` 없음 |
| `ui/src/**` | 문자열 `"billing"`·`"pricing"` 검색 | 0건 — 결제 UI 없음 |

### 1.2 인용 가능한 사실

1. 계정 서비스 모듈 목록은 6개로 고정돼 있다 — `account/mod.rs:1-6`. 저장소는 단일 JSON이고 `AccountStore`는 7개 맵(`users/sessions/login_codes/enrollment_codes/machines/grants/device_auths`, store.rs:133-148)이며 전 필드에 `#[serde(default)]`(135·137·139·141·143·145·147)가 붙어 **필드 추가는 기존 파일과 하위호환**이다.
2. 모든 쓰기는 read-modify-write + flock이다. `AccountState::mutate`(service.rs:193-205)가 `lock_account_dir`(197) → `load`(190) → `purge_expired` → 변경 → `save`를 한 임계구역으로 묶는다. 락은 `account-store.lock`의 `file.lock()`(store.rs:195-213), 저장은 temp + `0o600` + `fsync` + `rename`(store.rs:216-236).
3. 라우터 진입점은 `pub fn router(state: Arc<AccountState>) -> Router`(service.rs:1111)이고 라우트 13개가 1113-1137행 한 블록에 모여 있다. 결제 라우트는 여기에 추가한다.
4. 릴레이는 Ed25519 **고정 공개키 검증**만 안다(도메인 분리 account_grants.rs:14, 서명 입력 26행, pinned key 검증 38행, GrantGate 라우터 89행). 구독 상태를 조회하지 않고 `AccountState`(service.rs:32-44)에도 결제 설정 필드가 없다. 따라서 결제 상태는 **account 서비스**(`ferryx-account`, bin/account.rs:46-48)에만 두고, 릴레이는 `verify_grant_signature`(account_grants.rs:38) 결과만 신뢰하며 클라이언트는 계정 API가 발급한 grant/entitlement만 본다 — 결제는 릴레이·데스크톱 코드에 새 의존성을 만들지 않는다.

## 2. Merchant of Record 결정

### 2.1 비교 (2026-09-25 검색 기준)

| 기준 | Paddle | Lemon Squeezy | Stripe(+Tax/Managed Payments) | Polar |
|---|---|---|---|---|
| MoR·세금 대행 | MoR, VAT/부가세 계산·신고·납부 대행 | MoR, 동일 | 기본 PSP(판매자가 MoR), Managed Payments만 대행 + 자격 심사 | MoR, 계산·납부 대행 |
| 한국 사업자/개인사업자 | 판매자 국가 제한 없음(제재국 제외), 법인 없이 개인 계약 가능 | 계약 가능 | 한국 사업자 지원이 제한적이라는 실무 보고, Managed Payments는 사업장 소재지 심사 | 계약 가능(규모 작음) |
| 수수료 | 5% + $0.50/트랜잭션, 월 고정비 없음 | 5% + $0.50 | 처리 2.9% + $0.30 + MoR 3.5% 가산 | 기존 4% + $0.40, 2026-05 이후 신규 5% + $0.50 |
| 구독/체험 | 구독·체험·쿠폰·프로레이션 | 동일 계열 | 최강(수동 청구·인보이스) | 구독·체험·사용량 과금 |
| 웹훅 품질 | `Paddle-Signature`, 재시도, 이벤트 조회 API | 서명 + 재시도 | `Stripe-Signature` + 재시도 + 로컬 CLI | 서명 + 재시도 |
| API 성숙도 | 높음 | 중간(Stripe 인수 후 로드맵 불확실) | 최고 | 낮음~중간 |
| 환불/차지백 | MoR이 응대·차지백 커버 | 동일 | dispute 워크플로를 우리가 운영 | 응대는 우리 몫 |
| 정산 주기 | 월 정산(임계값 도달 시) | 월 정산 | 롤링 정산(한국 계정 조건 확인 필요) | 월 정산, 늦음 |

출처: comparedge.com/tools/paddle/pricing · fees.tools/paddle-fee-calculator · paddle.com/help/start/intro-to-paddle/which-countries-are-supported-by-paddle · help.boathouse.co/guides/beginners-guide-to-paddle/faq-can-i-sell-via-paddle-as-an-individual · romow.com/tool/paddle · hyunjoong.kim/en/blog/paddle-payment-guide · getstacksmart.com/blog/lemon-squeezy-merchant-of-record-fees-2026 · whatpayment.com/en/reviews/lemonsqueezy-review · agent.mue.app/news/stripe-managed-payments-own-merchant-of-record-2026 · docs.stripe.com/payments/managed-payments/eligibility · polar.sh/resources/pricing · paritydeals.com/polar-fee-calculator

### 2.2 권고 — Paddle 주력, Lemon Squeezy 폴백

1. **개인사업자/개인 신분으로 시작 + 한국 세무 리스크 제거** — 법인 없이 계약 가능하고 판매자 국가 제한이 없어 한국 판매자의 Stripe 제약(법인·심사)을 우회하는 가장 짧은 경로이며, MoR이 VAT/부가세 계산·신고·납부를 대행하므로 해외 VAT 등록이 필요 없다.
2. **운영비 절감이 수수료 차이보다 크다** — 차지백 응대·구매자 CS·환불이 요금에 포함된다. 1인 조직에서 이 인건비는 2-3%p 차이보다 크다.
3. **비용 구조 단순·웹훅 성숙** — 5% + $0.50 고정, 월 고정비 없음. Stripe는 PSP + MoR 가산 + 자격 심사로 불확실성이 크고, Paddle의 서명·재시도·이벤트 조회는 이미 쓰는 "서명 검증 후 상태 전이" 패턴(account_grants.rs:38)과 맞물린다.

폴백: 심사 거절·한국 정산 경로 차단 시 **Lemon Squeezy**(동일 요율, Stripe 인수로 인프라 안정). Stripe는 (1) 미국/싱가포르 법인을 세우고 (2) 인보이스·세금계산서가 필요한 B2B가 주가 된 시점에만 재검토하고, Polar는 요율 인상 이력(2026-05)·운영 도구 성숙도 때문에 후순위다.
요금제 제약: `5% + $0.50`은 저가 플랜에서 치명적이다(월 $8 → 실효 11.25%, 월 $12 → 9.2%). 최저 플랜은 **월 $12 이상** 또는 **연간 결제(2개월 무료)** 기본 노출로 잡고, 트랜잭션당 $0.50이 붙으므로 월간 갱신을 쪼개지 않는다.

## 3. 트라이얼 설계

### 3.1 MoR-side trial vs self-managed 30일 — 권고: self-managed 무카드 30일

- MoR-side trial: 상태를 provider가 소유(`trialing` → 자동 `active`/`past_due`), **카드 필수**, 종료 시 자동 청구, 어뷰즈는 낮지만 요금제·통화 변경이 provider 정책에 종속된다.
- self-managed 30일: 상태를 `AccountStore`가 소유, **무카드**, D-7/3/1 넛지 후 수동 결제, 이전이 자유로운 대신 이메일 재가입 어뷰즈가 있다. 1차 고객인 개발자·에이전트 파워유저에게 "결제수단 등록"은 최대 이탈 요인이므로 무카드를 택하고, 어뷰즈는 (a) 체험 시작은 `user_id`당 1회 영구 기록, (b) 동일 이메일 도메인·기기 지문으로 중복 계정 표시, (c) 체험판 **머신 2대 · 릴레이 동시 1세션**, (d) 체험 종료 시 릴레이만 잠그고 로컬 터미널·데몬은 건드리지 않는다(무료 티어 강등)로 억제한다.

### 3.2 체험 종료 전 과금 방지

- 체험 기간에는 **provider 쪽에 구독을 만들지 않는다.** 스토어에 `SubscriptionRecord.status = Trialing`만 있고 `provider_subscription_id`는 `None`이다.
- 결제는 `POST /api/account/v1/billing/checkout` 시점에만 시작하고 provider 트랜잭션의 `customData: { userId }`로 계정과 연결한다. provider에 체험을 만들지 않으므로 **자동 청구 사고가 구조적으로 불가능**하고, 대신 D-7/D-3/D-1 넛지를 `Mailer` 추상화(service.rs:15) 확장으로 보낸다.

### 3.3 카드 업프론트 vs 무카드

- 무카드는 진입 장벽 0으로 상단 퍼널이 크고 카드 이탈이 체험 종료 시점으로 이동한다 — 유료 전환율은 낮게 측정되지만 **모집단이 커서 절대 전환자 수가 크다**. 카드 업프론트는 전환율이 높은 대신 체험 시작이 줄고 카드 만료·3DS 실패·지역 거절이 CS로 유입되며, MoR이 카드 처리를 대행해도 **카드 폼 요구 순간 이탈**한다. 결론은 무카드이며, **체험 만료 시에는 3일 read-only grace 후 신규 원격 연결을 차단**한다(§6.4 dunning grace와 별개). 이 3일은 문서 전체의 단일 값이며 `trial_ends_at + 3d`로 `trials` 레코드에서 계산된다(레이 04 D4와 동일).

## 4. 데이터 모델 (`AccountStore` 확장)

`store.rs:133`의 `AccountStore`에 맵 4개를 추가한다. 전 필드 `#[serde(default)]` 규약(store.rs:134-147)을 따르므로 구버전 JSON은 빈 맵으로 읽힌다.

```rust
// src-tauri/src/account/store.rs — AccountStore 에 추가
pub billing_customers: BTreeMap<String, String>,           // user_id → provider_customer_id
pub subscriptions: BTreeMap<String, SubscriptionRecord>,   // key: provider_subscription_id
pub payment_events: BTreeMap<String, PaymentEventRecord>,  // key: provider event_id (멱등 키)
pub invoices: BTreeMap<String, InvoiceRecord>,             // key: provider invoice_id

// 세 레코드 공통: #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)] + #[serde(rename_all = "camelCase")]
pub struct SubscriptionRecord {
    pub subscription_id: String,        // = provider_subscription_id
    pub user_id: String,
    pub provider: String,               // "paddle" | "lemonsqueezy" | "polar"
    pub provider_customer_id: String,
    pub plan: String,                   // "pro" | "team"
    pub status: SubscriptionStatus,     // trialing | active | past_due | paused | canceled
    pub seats: u32,                     // 허용 machine 수
    pub current_period_start: u64,
    pub current_period_end: u64,
    pub trial_ends_at: Option<u64>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<u64>,
    pub updated_at: u64,                // provider 발생 시각 — 순서 역전 판정
}
pub enum SubscriptionStatus { Trialing, Active, PastDue, Paused, Canceled }
pub struct PaymentEventRecord {
    pub event_id: String,               // provider event id — 멱등성의 유일한 근거
    pub event_type: String,             // "subscription.activated" 등 원문 타입
    pub subscription_id: Option<String>,
    pub received_at: u64,
    pub applied_at: Option<u64>,        // None 이면 리플레이 대상
    pub outcome: String,                // "applied" | "ignored" | "duplicate" | "rejected:<code>"
    pub payload_sha256: String,         // 원문 대신 해시만 보관
}
pub struct InvoiceRecord {
    pub invoice_id: String,
    pub subscription_id: String,
    pub user_id: String,
    pub status: String,                 // "paid" | "past_due" | "refunded"
    pub total_cents: i64,
    pub currency: String,
    pub billing_reason: String,         // "initial" | "renewal" | "manual"
    pub period_start: u64,
    pub period_end: u64,
    pub refunded_cents: i64,
    pub issued_at: u64,
}
```

`purge_expired`(store.rs:166-172)에 두 규칙을 추가한다: `payment_events`는 `applied_at + 90일` 뒤 정리하고 `invoices`는 정산·세무 추적을 위해 삭제하지 않는다(`subscriptions`는 `canceled_at + 180일` 보존). 권한 파생은 단일 진실 공급원 하나로 모은다:

```rust
// src-tauri/src/account/billing.rs (신규)
pub fn derive_entitlements(store: &AccountStore, user_id: &str, now: u64) -> Entitlements;
pub struct Entitlements { plan: String, relay_enabled: bool, max_machines: u32, max_concurrent_remote: u32,
                          trial_active: bool, trial_ends_at: Option<u64>, grace_until: Option<u64>, paid_through: u64 }
pub enum EntitlementDenial { NoSubscription, GraceExpired, SeatLimit, Canceled }
```

`derive_entitlements`는 **순수 함수**(스토어 + `now`)여야 한다. 시각을 인자로 받으므로 시간 이동 테스트가 자유롭고 웹훅·라우트·CLI가 같은 판단을 공유한다. `grace_until`은 `past_due`일 때만 `Some`이다.

## 5. 웹훅 설계

### 5.1 경로

`POST /api/account/v1/billing/webhook/{provider}` (`provider` = `paddle` | `lemonsqueezy` | `polar`)를 `service.rs:1111 router()` 블록에 추가한다. 같은 Axum 서버(기본 바인드 127.0.0.1:43822, bin/account.rs:24)를 쓰고 인증은 세션이 아니라 **서명**이다 — `user_for_session`(store.rs:178)을 태우지 않는다. 본문 제한은 `DefaultBodyLimit::max(limit)`(service.rs:1136) 정책을 따르되 결제 웹훅만 256 KiB로 올린다.

### 5.2 서명 검증 — 이미 있는 패턴 재사용

선례는 릴레이의 계정 서명 검증(account_grants.rs:38)이다: **도메인 분리**(14행 `GRANT_SUBMISSION_DOMAIN`), **원문 바이트 그대로 사용**(26행, 테스트 스텁 `|body: Bytes|` service.rs:1424), **실패는 닫힘**(키 없음·불일치 모두 `FORBIDDEN`, 38-62행).

1. 핸들러는 원문을 `axum::body::Bytes`로 받고 **파싱 전에** 서명을 검증한다. Paddle은 `Paddle-Signature: ts=<unix>;h1=<hex>` → 대상 `<ts>:<raw_body>`, HMAC-SHA256, 5분 이내 타임스탬프만 허용한다.
2. 로컬 개발·테스트도 같은 스킴의 자체 헤더 `x-ferryx-billing-signature: t=<unix>,v1=<hex(hmac_sha256("t.body"))>`를 써서 단위 테스트와 curl 스모크가 같은 코드를 태우게 한다. 비교는 상수시간(`hmac` crate `verify_slice`)이며, `hmac`이 현재 의존성에 없으므로(Cargo.toml:1-205) **P1에서 `hmac = "0.12"` 추가**가 유일한 신규 의존성이다(`sha2`는 store.rs:8에서 이미 사용).

### 5.3 멱등성

- 멱등 키는 **provider event id** 하나다. `payment_events[event_id]`가 있으면 상태를 건드리지 않고 `200 {"status":"duplicate"}`를 반환하고, 순서가 뒤바뀐 이벤트는 provider 발생 시각이 더 낮으면 `ignored`로 기록·폐기한다(`updated_at` 비교).
- 판정과 적용은 같은 락 안에서 한다: `AccountState::mutate`(service.rs:193-205, lock 197)가 flock → `load` → 변경 → `save`(store.rs:216-236 원자적 rename)를 한 임계구역으로 묶으므로 별도 중복 검사가 필요 없다.

### 5.4 이벤트 → 상태 전이표

| provider 이벤트 | `SubscriptionRecord` 변화 | 파생 동작 |
|---|---|---|
| `subscription.created` / `subscription.trialing` | upsert, `status=trialing|active`, `price_id→plan` | `billing_customers` 매핑, 체험 넛지 등록 |
| `subscription.activated` | `status=active`, `current_period_*` 갱신 | 권한 재계산, 릴레이 허용 |
| `subscription.past_due` | `status=past_due` | `grace_until = now + 7일`, 넛지 메일 (체험 grace 3일과 별개 수치) |
| `subscription.canceled` / `subscription.paused` | `status=canceled|paused`, `canceled_at` | 기간 만료까지 유지 후 `relay_enabled=false`(paused는 즉시 신규 세션 차단, 기간 보존) |
| `transaction.completed` / `invoice.paid` | — | `InvoiceRecord.status=paid`, `paid_through` 갱신 |
| `transaction.payment_failed` | (미설정이면) `status=past_due` | §6.4 dunning |
| `adjustment.created`(환불) | — | `refunded_cents` 누적, §6.6 정책 훅 |
| `subscription.updated` | `seats`/`plan`/`period` 갱신 | 좌석 축소 시 초과 머신 정리 안내 |

provider별 실제 이벤트명은 구현 시 매핑 테이블로 고정하고, 표에 없는 타입은 `ignored`로 남겨 감사 가능하게 한다.

### 5.5 실패한 웹훅의 안전한 재시도

- **200**: 적용 완료·중복·무시(미지원 타입) — provider 재시도를 멈춘다. **401**: 서명 불일치 — `rejected:signature`로 기록하고 401을 반환해 provider 대시보드에 실패가 남게 한다. **409**: `customData.userId`가 스토어에 없어 매핑 실패 — 사람이 봐야 하는 상태이며 200으로 삼키지 않는다. **5xx**: 락·쓰기 등 일시 오류 — `applied_at=None`으로 남기고 provider 지수 백오프 재시도를 기다린다.
- **리플레이**: `applied_at=None` 이벤트를 `GET /api/account/v1/billing/health`(운영자 전용, account 서명 요청만)에 노출하고 `POST /api/account/v1/billing/webhook/replay { eventId }`로 멱등 재적용한다. 원문을 저장하지 않으므로 재적용은 파생 필드 갱신으로 제한되고, 원문 재검증이 필요하면 provider 이벤트 조회 API를 호출한다.

## 6. 고객 라이프사이클

### 6.1 체크아웃 (로그인 상태)

- `POST /api/account/v1/billing/checkout { plan, interval }` — `user_for_session`(store.rs:178)으로 `user_id`를 얻고, 서버가 provider 트랜잭션을 만들어 `{ url, transactionId }`를 반환한다(`customData`에 `userId`만 실음). 사용자는 앱 내 WebView로 URL을 열고 복귀는 앱 딥링크(`ferryx://billing/return`)로 받으며, 권한은 **웹훅으로만** 켜고 리다이렉트 성공 화면은 근거로 삼지 않는다(위조 가능).

### 6.2 업그레이드 / 다운그레이드 / 취소

| 동작 | 라우트 | 구현 |
|---|---|---|
| 업그레이드 | `POST /api/account/v1/billing/change-plan { plan:"team" }` | provider 구독 항목 교체(즉시, 프로레이션) → `subscription.updated`가 확정 |
| 다운그레이드 | 같은 라우트 + `"effectiveAt":"period_end"` | 기간 말 적용 예약 |
| 취소·즉시 해지 | `POST /api/account/v1/billing/cancel { atPeriodEnd }` | `true`면 예약 취소 후 `canceled` 수신 시 `canceled_at`, `false`면 §6.6 정책 확인 후 provider 즉시 취소 |

라우트 응답은 `202 { pending: true }`이고 UI는 다음 `entitlements` 조회 결과로만 갱신한다(낙관적 갱신 금지).

### 6.3 머신 수 변경

- `seats`가 허용 machine 상한이다. 게이트는 **grant 발급 지점**: `POST /api/account/v1/machines/{machine_record_id}/grants`(service.rs:1132-1135)와 `machines/enroll`(service.rs:1131)에서 `max_machines` 초과 시 `402 PAYMENT_REQUIRED { code:"SEAT_LIMIT_REACHED" }`(`ApiError` 확장, service.rs:216-240).
- 좌석 축소(Team → Pro)는 **기존 머신을 지우지 않는다.** 초과분은 `relay_enabled=false`로 표시만 하고 사용자가 남길 머신을 고른다. 로컬 데몬·PTY는 라이선스와 무관하게 계속 산다(데몬 종료로 세션을 잃지 않는다는 기존 규칙).

### 6.4 dunning과 grace

- `transaction.payment_failed` → `status=past_due`, `grace_until = received_at + 7일`. grace 7일 동안 릴레이 **신규 세션 허용 유지**, 넛지 D+1/D+3/D+7, 앱 배너.
- grace 만료 → `relay_enabled=false`. 릴레이는 세션ID를 수락하기 **전에** 거절하므로(relay_server.rs:26-28 "Unknown IDs are rejected before upgrade") **진행 중인 원격 세션은 끊지 않고** 새 attach만 차단한다.
- `invoice.paid` 수신 시 즉시 `active` 복귀, `grace_until=None`. 재개는 멱등하며 재허용은 클라이언트 캐시 TTL(권장 60초)만큼 지연될 수 있다. CS는 스토어 JSON의 `grace_until`만 보고 답한다.

### 6.5 past_due에서 릴레이 접근

past_due는 **릴레이(원격 접속)만** 잠근다. 로컬 터미널·워크트리·에이전트 실행은 무료 티어로 계속 동작한다. 이유: (a) 데몬은 사용자 로컬 자산이고 종료하면 세션·에이전트 작업이 죽는다, (b) 잠금 강도가 낮을수록 CS·분쟁이 줄고 재결제율이 오른다. 유일한 예외는 환불·차지백이다(§6.6).

### 6.6 환불 정책 훅

- 정책: 첫 결제 후 14일 이내 전액 환불, 그 외에는 기간 말 종료(부분 환불 없음). MoR이 응대하므로 코드는 **결과만 반영**한다. 훅은 `adjustment.created`/`refund.created` → `refunded_cents` 누적 → 전액이면 `status=canceled` + `relay_enabled=false`(grace 없음), 부분이면 상태 유지 + 로그이고, 차지백(`dispute.created`)도 즉시 잠금 + 운영 알림이며 자동 재개는 없다(수동 해제 `POST /api/account/v1/billing/dispute/resolve`, account 서명 검증).

## 7. 결정 기록 (Decisions)

- **D1. Merchant of Record** — (a) Paddle (b) Lemon Squeezy (c) Stripe Managed Payments (d) Polar 중 **권고 (a) Paddle**, 심사 실패 시 **(b) Lemon Squeezy**(§2.2). 기각: Stripe는 한국 판매자 자격·법인 요건 불확실(§2.1), Polar는 요율 인상 이력·도구 성숙도.
- **D2. 카드 업프론트** — (a) 카드 업프론트 14일 (b) 무카드 30일 + 종료 시 릴레이 잠금 중 **권고 (b)**(§3.3). 어뷰즈 대책은 카드가 아니라 `user_id`당 1회 + 머신 2대 + 릴레이 동시 1세션.
- **D3. grace 길이** — (a) 3일 (b) 7일 (c) 14일 중 **권고 (b) 7일**. 7일은 카드 교체·법인 카드 승인을 덮고, 14일은 정산 주기가 월 단위라 사실상 한 달 무료 연장이 된다.
- **D4. 강제 집행 지점** — (a) 릴레이 (b) account 서비스 grant 발급 중 **권고 (b)**. 릴레이는 서명만 검증하고 구독을 모른다(account_grants.rs:38, 89).

## 8. 단계 계획 (Phase plan)

| 단계 | 산출물 | 구체 경로/함수 |
|---|---|---|
| P1 기반 | 레코드·권한 파생·HMAC | `store.rs:133`에 맵 4개, `store.rs:166-172 purge_expired` 확장, `src-tauri/src/account/billing.rs` 신규(`derive_entitlements`·`verify_webhook_signature`·`apply_event`), `account/mod.rs:1-6`에 `pub mod billing;`, `Cargo.toml`에 `hmac = "0.12"` |
| P2 웹훅 | 라우트·검증·전이 | `service.rs:1111 router()`에 `/billing/webhook/{provider}`·`/billing/entitlements`·`/billing/checkout` 추가, `AccountState`(service.rs:32-44)에 `billing: BillingConfig` 필드, 환경변수는 `origin.rs` 패턴(`FERRYX_BILLING_PROVIDER`·`FERRYX_BILLING_WEBHOOK_SECRET`) |
| P3 권한 게이트 | 좌석·체험 만료 강제 | `issue_grant`(service.rs:1132-1135)와 `machines/enroll`(service.rs:1131)에서 `derive_entitlements` 검사, `ApiError`(service.rs:216-240)에 `PAYMENT_REQUIRED`·`SEAT_LIMIT_REACHED` |
| P4 클라이언트·운영 | 체크아웃·상태 표시·CS | `src-tauri/src/account/billing_client.rs` 신규(`enroll_client.rs` 옆), `ui/src/components/` Billing 섹션 + `ferryx://billing/return` 딥링크, 리플레이 라우트, `payment_events.applied_at=None` 대시보드, 환불·차지백 런북 |

각 단계는 이전 단계 테스트가 초록일 때만 진행한다. P1·P2는 릴레이·데스크톱 코드를 건드리지 않는다(§1.2-4).

## 9. 수용 테스트 (acceptance tests)

**T1. 웹훅 멱등성** — `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::duplicate_webhook_applies_once` → 같은 `event_id` 2회 적용 시 `payment_events.len() == 1`, `subscriptions["sub_1"].status == Active`, 2회차 응답 `{"status":"duplicate"}`.

**T2. 서명 변조 거절** — `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::tampered_signature_is_rejected` → 본문 1바이트 변조 시 `401`, `payment_events`에 `rejected:signature` 1건, `subscriptions` 불변.

**T3. grace → 릴레이 잠금** — `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::past_due_grace_then_relay_denied` → `failed_at + 6일`에서 `relay_enabled == true`, `+8일`에서 `false`, `invoice.paid` 적용 후 다시 `true`.

**T4. 좌석 초과 거절** — `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::grant_issuance_is_capped_by_seat_entitlement` → `seats: 1` 구독에서 두 번째 머신 grant 발급 시 `402` + `SEAT_LIMIT_REACHED`.

**T5. 실서버 curl 스모크 (서명된 웹훅)**

```bash
export FERRYX_ACCOUNT_BIND=127.0.0.1:43822 FERRYX_BILLING_WEBHOOK_SECRET=dev-secret
cargo run --manifest-path src-tauri/Cargo.toml --bin ferryx-account &
BODY='{"event_id":"evt_1","event_type":"subscription.activated","data":{"subscription_id":"sub_1","user_id":"u1","plan":"pro","seats":1,"current_period_end":1790000000}}'
T=$(date +%s); V1=$(printf '%s.%s' "$T" "$BODY" | openssl dgst -sha256 -hmac dev-secret -hex | awk '{print $2}')
curl -sS -i -X POST http://127.0.0.1:43822/api/account/v1/billing/webhook/paddle \
  -H "x-ferryx-billing-signature: t=$T,v1=$V1" -H 'content-type: application/json' --data-raw "$BODY"
```

관찰값: 1회차 `200 {"status":"applied"}`, 같은 명령 재실행 `200 {"status":"duplicate"}`, 서명 한 글자 변조 `401`, `~/.ferryx/account/account-store.json`에 `paymentEvents.evt_1`이 정확히 1건.

**T6. 회귀 안전** — `cargo test --manifest-path src-tauri/Cargo.toml --lib account::` 및 `remote::account_grants`가 전부 초록. 스토어 하위호환은 기존 라운드트립 테스트(store.rs:244)가 지킨다.

## 10. 리스크 / 열린 질문

- **수수료 변동성·한국 세무** — Polar는 2026-05에 요율을 올렸으니 계약 전 요율·정산 주기를 재확인하고 `{provider}` 추상화(§5.1)로 교체 비용을 낮춘다. MoR이 부가세를 대행해도 매출 인식·원천세 신고 의무는 남으므로 세무사 확인이 필요하다(열린 질문).
- **체험 어뷰즈·순서 역전·캐시 지연** — 무카드 체험은 이메일 재가입에 취약하므로 릴레이 동시 1세션을 방어선으로 두고 필요하면 체험을 14일로 줄인다. §5.3의 발생 시각 비교를 반드시 구현하고, grace 만료 후 클라이언트 캐시(60초) 동안 원격 접속이 유지될 수 있음을 허용 트레이드오프로 명시한다(필요 시 grant TTL, service.rs:30의 600초 조정).

</details>

### 3.8 이메일 흐름 (Resend)
- 가입/체험/만료/영수증/결제실패/해지/보안 알림 템플릿 → 상세: `04-email-account-flows.md`
- 핵심 계약(요약): 기존 `Mailer`(`mailer.rs:31`)에 템플릿 종류를 확장한다. 발송 실패는 결제 상태를 바꾸지 않는다(웹훅이 진실).


<details>
<summary>레인 상세 원문 (04-email-account-flows.md)</summary>

# 04. 이메일 · 계정 라이프사이클 플로우 (Email & Account Lifecycle Flows)

> Ferryx 상업 모델(relay 라이선싱) 계획의 04번 섹션. 모든 "현재" 서술은 `file:line` 로 인용했고, 아직 없는 것은 **신규** 로 표시했다.

#### 0. 현재 구현 (as-is)

##### 0.1 토폴로지 — 계정 서비스는 relay 안에 있다
- 계정 서비스는 **relay 프로세스에 임베드**된다: `src-tauri/src/bin/relay.rs:147` 에서 `create_production_mailer(Some(data_dir.join("mail")))` 로 메일러를 만들고, `:149` 에서 `AccountState::new(&data_dir, &origin, mailer)`, `:153` 에서 `relay_router_with_account(...)` 로 라우터를 조립한다. 실제 병합 지점은 `src-tauri/src/remote/relay_server.rs:2233` 의 `router.merge(crate::account::service::router(account))`.
- 계정 라우트는 `/api/account/v1/*` **전체 경로**가 라우터에 박혀 있다(`src-tauri/src/account/service.rs:1114-1134`). 메일이 싣는 매직링크(`service.rs:603`)와 디바이스 approve URL(`service.rs:442-444`)도 이 origin 을 그대로 쓴다.
- 독립 실행 경로도 있다: `ferryx-account` 바이너리(`src-tauri/Cargo.toml:31-33`)는 기본 바인드 `127.0.0.1:43822`(`src-tauri/src/bin/account.rs:21-24`)로 뜨고 **FileMailer 고정**(`src-tauri/src/bin/account.rs:34`)이다. 이 경로로 메일을 내보내려면 `FERRYX_MAIL_DIR` 이 있어야 한다(`mailer.rs:54`).

##### 0.2 메일러 — 지금은 메일 종류가 1개뿐이다
- `Mailer` 트레이트(`src-tauri/src/account/mailer.rs:30-31`)의 메서드는 `send_magic_link(to, url)` **하나뿐**이다. 즉 §1 의 letter 카탈로그는 전부 신규다.
- `ResendMailer` 가 `POST https://api.resend.com/emails` 로 전송한다(`mailer.rs:183`). 페이로드는 `mailer.rs:147-154` 의 **인라인 HTML 한 벌**이고 제목은 `"Sign in to Ferryx"`(`mailer.rs:151`) 로 고정이다.
- 폴백은 `WebhookMailer`(`mailer.rs:255-259`)와 `FileMailer`(`mailer.rs:54`, 디렉터리에 URL 1줄 기록, 0600). 선택 순서는 `create_production_mailer`(`mailer.rs:247-264`): `RESEND_API_KEY` → `FERRYX_MAIL_WEBHOOK_URL` → FileMailer.
- 실패 코드는 `MAIL_FAILED` 하나(`mailer.rs:11`)이고, HTTP 로는 503 으로 매핑된다(`service.rs:606-610`).

##### 0.3 mailer 가 읽는 환경변수 (전수)
| env | 위치 | 의미 |
|---|---|---|
| `RESEND_API_KEY` | `mailer.rs:248` | 값이 있으면 Resend 경로 선택 |
| `FERRYX_MAIL_FROM` | `mailer.rs:250-251` | From; 미설정 시 기본값 `Ferryx <login@checka.cc>` |
| `FERRYX_MAIL_WEBHOOK_URL` | `mailer.rs:255` | Resend 키가 없을 때 쓰는 웹훅 전송 URL |
| `FERRYX_MAIL_WEBHOOK_TOKEN` | `mailer.rs:257` | 웹훅 Bearer 토큰(선택) |
| `FERRYX_MAIL_DIR` | `mailer.rs:54` | FileMailer 기록 디렉터리; 없으면 `MAIL_FAILED` |

계정/relay 보조 변수: `FERRYX_ACCOUNT_ORIGIN`(`account/origin.rs:16`), `FERRYX_ACCOUNT_DATA_DIR`(`origin.rs:38`), `FERRYX_ACCOUNT_ORIGIN_ALLOW_FERRYX_DEV`(`origin.rs:70`), `FERRYX_ACCOUNT_BIND`(`bin/account.rs:21`), `FERRYX_ACCOUNT_LOGIN_PER_HOUR`(`bin/account.rs:26`), `FERRYX_ACCOUNT_MAX_BODY_BYTES`(`bin/account.rs:30`), `FERRYX_ACCOUNT_RELAY_ORIGIN`(`bin/relay.rs:128`). relay 의 기본 origin 은 `https://relay.checka.cc`(`bin/relay.rs:126-127`).

##### 0.4 로그인 · 디바이스 · 등록 (현재 플로우)
- 매직링크 요청: 라우트 `service.rs:1115`, 핸들러 `service.rs:581-626`. 이메일 형식만 확인하고(`service.rs:587-592`) **사용자 조회 없이** `202 ACCEPTED`(`service.rs:625`)를 돌려준다. 코드는 SHA-256 해시로 저장(`service.rs:617`, `token_hash` `store.rs:27-33`), TTL `LOGIN_CODE_TTL = 600s`(`store.rs:10`).
- 매직링크 소비: `service.rs:628-673`. 미사용 코드는 `LOGIN_CODE_USED`(`service.rs:637`), 만료는 `LOGIN_CODE_EXPIRED`(`service.rs:642`). 사용자 레코드는 여기서 **lazy 생성**(`service.rs:648-656`), 세션 TTL `SESSION_TTL = 30d`(`store.rs:11`).
- 디바이스: `/device/request` `service.rs:415-474` — `user_code` `XXXX-XXXX`(`service.rs:430-437`), `verification_uri = {origin}/device`(`service.rs:439`), 메일 링크 `{origin}/api/account/v1/device/approve?code=..&token=..`(`service.rs:442-444`), 만료 900초(`service.rs:450`), 폴링 `interval = 2`(`service.rs:472`). `/device/poll` `service.rs:476-505`(`authorization_pending` `service.rs:500`), `/device/approve` `service.rs:507-579` 는 email token 이 없거나 틀리면 거부하고(`service.rs:527`, `service.rs:554`) 사용자를 lazy 생성한 뒤(`service.rs:538`) enrollment code 를 발급한다(`service.rs:557-569`).
- 등록: `/enrollment-codes` `service.rs:689-708`(`ENROLLMENT_CODE_TTL = 600s` `store.rs:12`, `service.rs:695`), challenge `service.rs:767-780`, `/machines/enroll`(라우트 `service.rs:1131`). 이미 등록된 머신은 `enrollment_epoch + 1` 로 갱신(`service.rs:853-880`), 타 계정 소유 머신은 `ACCOUNT_MACHINE_CLAIMED`(CONFLICT). grant 발급은 `service.rs:913`+, `GRANT_TTL = 600s`(`service.rs:30`).
- rate limit: 주소별 **시간당 5회**(`DEFAULT_LOGIN_REQUESTS_PER_HOUR`, `store.rs:13`), 프로세스 로컬 in-memory window(`service.rs:149-159`) → `429 LOGIN_RATE_LIMITED`(`service.rs:595-599`). 본문 상한 4096바이트(`store.rs:14`, `service.rs:1136`).
- 상태 저장: 전부 `account-store.json` 한 파일(`store.rs:187-188` 의 `store_path`) — map 7종(`store.rs:132-147`), flock(`store.rs:195`) + tmp rename(`store.rs:216`), 갱신은 `mutate` 한 곳(`service.rs:193-204`: lock `:197`, `purge_expired` `:200`, `save` `:202`). `purge_expired`(`store.rs:166-172`)는 login_codes 를 **지우지 않는다** — consume 이 만료를 구분할 수 있게 하려는 의도다(`store.rs:167-171`).
- **현재 없는 것**: `src-tauri/src/account/` 에 `trial` · `stripe` · `license` · `entitlement` 식별자가 0건이고, 머신 개수 상한(`max_machines`)도 0건이다(`subscription` 은 `ssh/`·`daemon/` 의 pub/sub 스트림에만 등장). 상업 라이프사이클은 전부 신규다.

#### 1. 상업 모델이 필요한 이메일 (letter 카탈로그)

템플릿 규약(신규): 본문은 `src-tauri/src/account/templates/<letter_id>.html`, 제목·필수 변수는 `src-tauri/src/account/mail_templates.rs` 의 `MailLetter::subject(locale)` / `MailLetter::required_vars()` 가 **코드로** 소유한다. 변수 치환은 `{{snake_case}}` 로 통일하고 렌더러는 미치환 변수를 만나면 실패시킨다(빈칸 발송 금지).

| letter_id | 트리거 (현재 위치 / 신규) | 수신자 | 제목 KO / EN | 변수 | 템플릿 경로 |
|---|---|---|---|---|---|
| `login_magic_link` | `POST /login/request` `service.rs:581` (현재 `send_magic_link` 호출 `service.rs:604`) | 요청 주소 | `Ferryx 로그인 링크` / `Sign in to Ferryx` (현재 고정값 `mailer.rs:151`) | `url`, `origin`, `expires_minutes` | `templates/login_magic_link.html` (현재는 `mailer.rs:147-154` 인라인) |
| `device_approve` | `POST /device/request` `service.rs:415` (현재 전송 `service.rs:445-447`) | 승인 대상 계정 주소 | `Ferryx 기기 승인 요청` / `Approve this Ferryx machine` | `approve_url`, `user_code`, `expires_minutes`(=15), `platform` | `templates/device_approve.html` (현재는 로그인 메일 제목·본문 재사용) |
| `trial_started` | **신규**: 첫 `/machines/enroll` 성공 시(D1) — 라우트 `service.rs:1131` | 계정 주소 | `30일 체험이 시작되었습니다` / `Your 30-day trial has started` | `trial_ends_at`, `machine_name`, `plan_url`, `data_retention_days` | `templates/trial_started.html` |
| `trial_ending_soft` (D+23) | **신규**: lifecycle due-scan | 계정 주소 | `체험이 7일 남았습니다` / `7 days left in your trial` | `trial_ends_at`, `days_left`, `checkout_url`, `price_display` | `templates/trial_ending_soft.html` |
| `trial_ending_final` (D+29) | **신규**: lifecycle due-scan | 계정 주소 | `체험이 1일 남았습니다` / `1 day left in your trial` | 위와 동일 + `paywall_url` | `templates/trial_ending_final.html` |
| `trial_expired` | **신규**: `trial_ends_at` 도달(D+30) | 계정 주소 | `체험이 종료되었습니다` / `Your trial has ended` | `trial_ended_at`, `paywall_url`, `retained_until` | `templates/trial_expired.html` |
| `payment_receipt` | **신규**: billing webhook `invoice.paid` | 결제 계정 주소 | `Ferryx 결제 영수증` / `Your Ferryx receipt` | `invoice_id`, `amount_display`, `currency`, `paid_at`, `period_end`, `plan_name`, `receipt_url` | `templates/payment_receipt.html` |
| `payment_failed_d0/d3/d7` | **신규**: billing webhook `invoice.payment_failed` | 결제 계정 주소 | `결제에 실패했습니다` / `We couldn't process your payment` | `attempt_no`(0/3/7), `amount_display`, `next_retry_at`, `update_payment_url` | `templates/payment_failed.html` |
| `subscription_canceled` | **신규**: 취소 API 또는 provider `subscription.deleted` | 계정 주소 | `구독이 취소되었습니다` / `Your subscription is canceled` | `plan_name`, `access_until`, `resume_url` | `templates/subscription_canceled.html` |
| `machine_limit_reached` | **신규**: `/machines/enroll` 에서 상한 초과 | 계정 주소 | `기기 등록 한도에 도달했습니다` / `Machine limit reached` | `limit`, `current_count`, `manage_machines_url`, `plan_name` | `templates/machine_limit_reached.html` |
| `security_new_device` | **신규**: `/device/approve` 성공(`service.rs:507`) 또는 신규 enroll(`service.rs:853-880` 의 epoch 증가) | 계정 주소 | `새 기기에서 로그인했습니다` / `New device signed in` | `machine_name`, `platform`, `occurred_at`, `revoke_url` | `templates/security_new_device.html` |

운영 규칙: (a)(b) 두 통은 **거래성·1회용**이고 이미 코드 경로가 있다(`service.rs:604`, `service.rs:445-447`). (c)~(j) 는 전부 신규이며, 발송 시점은 §4 의 `mail_log` 로 1회만 보장한다. (d)(e)(g) 는 "돈이 걸린" 메일이므로 발송 실패를 `mail_log` 에 남기고 재시도한다(성공/실패 모두 기록).

#### 2. 전달성 · 남용 방지

- **도메인 인증**: 발신 도메인은 `FERRYX_MAIL_FROM`(`mailer.rs:250-251`)이 가리키는 도메인 하나이고, 전송 경로도 `mailer.rs:183` 한 곳이다. 따라서 SPF(Resend 포함)·DKIM(도메인 인증)·DMARC(`p=quarantine` 이상) 를 **도메인 단위로 한 번** 세팅하면 모든 letter 에 적용된다. `checka.cc` 를 계속 쓸 경우 기본 From 이 그 도메인이므로 DNS 작업 없이는 배포하지 않는다.
- **평판 격리**: 영수증·결제 실패·체험 종료는 거래성(transactional) 메일이다. 업셀/공지가 생기면 서브도메인과 List-Unsubscribe 를 분리하고, 거래성 메일에 마케팅 헤더를 붙이지 않는다(D3).
- **bounce / complaint**: **신규** `POST /api/account/v1/mail/events` 로 Resend 웹훅을 받아(서명 검증 필수) `AccountStore.mail_suppressions`(신규)에 등록한다. hard bounce 1회, soft bounce 3회 누적으로 차단하고, 차단된 주소로는 `mail_log` 에 `suppressed` 만 남긴다. 웹훅 본문은 본문 상한 4096바이트(`store.rs:14`) 안에 들어와야 하므로 envelope 만 받고 원문은 받지 않는다.
- **enumeration 방지**: 현재 `/login/request` 는 `email.contains('@')` 외에 store 를 조회하지 않는다(`service.rs:585-592`) — 존재/미존재 계정 모두 `202 ACCEPTED`(`service.rs:625`)다. `400`(`service.rs:587-592`)과 `429`(`service.rs:594-600`)도 계정 존재와 무관하다. 신규 letter 는 **인증된 세션**(`require_user` `service.rs:338`) 또는 **서명된 provider 웹훅**에서만 트리거하고, HTTP 응답 본문에 구독/체험 상태를 싣지 않는다(상태 조회는 `GET /api/account/v1/subscription`, bearer 필요).
- **요청 제한**: 주소별 시간당 5회(`store.rs:13`)가 이미 있으나 카운터가 **프로세스 로컬**(`service.rs:149-159`)이라 relay 인스턴스 수만큼(5×N) 늘어나고 재시작 시 초기화된다. 상업화 전에 store 기반 카운터로 승격한다(`login_attempts` 를 store record 로 이동, `purge_expired` `store.rs:166` 에 편입).
- **로그 위생**: 저장하는 것은 provider message id 와 letter id 뿐이다. 본문·매직링크 토큰은 저장하지 않는다(현재도 해시만 저장: `store.rs:57`, `service.rs:617`). 오픈 픽셀/추적 링크는 넣지 않는다.

#### 3. 텍스트 플로우

```
[signup]
  POST /api/account/v1/login/request   (service.rs:581)  ── mail: login_magic_link ─▶ 202 ACCEPTED (service.rs:625)
  POST /api/account/v1/login/consume   (service.rs:628)  ── UserRecord lazy 생성 (service.rs:648-656), session 30d (store.rs:11)
        │
        ▼
[machine enroll · trial start]                                           ← 결정 D1: 체험은 여기서 시작
  POST /device/request  (service.rs:415) ── mail: device_approve ─▶ 승인 (service.rs:507) ─▶ enrollment_code
  POST /machines/enroll (service.rs:1131) ─▶ MachineRecord(epoch 1 또는 +1, service.rs:853-880)
                                            + TrialRecord{started_at, ends_at = +30d}  + mail: trial_started
        │
        ▼
[trial 진행]
  grant 발급 (service.rs:913, TTL 600s service.rs:30) → 정상 사용
        ├─ D+23 ── mail: trial_ending_soft
        ├─ D+29 ── mail: trial_ending_final
        ▼
[trial end D+30]  TrialRecord.ends_at 도달 ── mail: trial_expired
        │
        ▼
[paywall]  relay 게이트: attach/grant 거부 + 402 PAYWALL_REQUIRED (신규). 계정·머신·기존 세션 데이터는 보존
        │
        ▼
[purchase]  POST /api/account/v1/subscription/checkout (신규) → provider 결제 → webhook invoice.paid (신규)
        │
        ▼
[receipt]  PaymentRecord + SubscriptionRecord.status = active  ── mail: payment_receipt
```

```
[cancel]
  POST /api/account/v1/subscription/cancel (신규) → SubscriptionRecord.cancel_at_period_end = true
        └── mail: subscription_canceled  (access_until = current_period_end)
             │
             ▼  current_period_end 까지 동일 권한 유지 (TrialRecord 는 재사용 불가 — 소진된 상태)
[end of period]
  SubscriptionRecord.status = canceled → paywall 재진입 (machine/grant/세션 데이터는 유지, 재구독 시 새 period 계산)
        └── mail: 재구독 시 payment_receipt / 결제 실패 시 payment_failed_d0 부터 다시 시작
```

멱등성 메모: 모든 전이는 §4 의 store 레코드 키로 판정한다. due-scan 은 store 만 보고 매 tick 재계산하므로 **relay 재시작으로 놓친 tick 은 다음 tick 에서 복구**된다(스케줄러 상태를 메모리에 두지 않는다).

#### 4. 상태가 사는 곳과 멱등성

기존 store 패턴(`store.rs:132-147`, `mutate` `service.rs:193-204`)을 그대로 확장한다. 신규 레코드는 전부 `#[serde(default)]` 로 추가해 구버전 store JSON 과 호환시킨다.

| 신규 레코드 | 키 | 필드 (핵심) | 쓰는 시점 |
|---|---|---|---|
| `trials` | `user_id` | `started_at`, `ends_at`, `started_machine_record_id` | 첫 enroll 성공(D1) |
| `subscriptions` | `user_id` | `plan_id`, `status(active/canceled/past_due)`, `current_period_end`, `cancel_at_period_end`, `provider_subscription_id` | checkout 완료 / webhook |
| `payments` | `invoice_id` | `user_id`, `amount_minor`, `currency`, `status`, `paid_at`, `period_end` | webhook `invoice.paid` |
| `dunning` | `user_id` | `attempt(0/3/7)`, `last_failed_at`, `next_retry_at` | webhook `invoice.payment_failed` |
| `mail_log` | `{user_id}:{letter_id}:{period_bucket}` | `sent_at`, `provider_message_id`, `status(sent/failed/suppressed)` | 모든 letter 발송 전후 |
| `mail_suppressions` | `sha256(email)` | `reason(hard_bounce/complaint)`, `created_at` | `POST /mail/events` |

| 플로우 | 진실 원천 | 멱등 키 / 중복 시 동작 |
|---|---|---|
| signup(매직링크) | `login_codes`(`store.rs:57`) | 코드 해시·1회용. 재사용은 `LOGIN_CODE_USED`(`service.rs:637`), 만료는 `LOGIN_CODE_EXPIRED`(`service.rs:642`) |
| device 승인 | `device_auths`(`store.rs:103`) | `user_code + email_token_hash`; 승인 후 poll 이 레코드를 지워 2회차부터 `DEVICE_CODE_INVALID`(`service.rs:485`) |
| enroll | `enrollment_codes`(`store.rs:64`) | 코드 해시 1회용 — 검증 후 삭제(`service.rs:844`), 만료 시에도 삭제(`service.rs:832`) + challenge nonce 1회용(`take_challenge`, `service.rs:181-188`) |
| trial 시작 | `trials` | 키 `user_id`. 이미 있으면 재시작 금지(두 번째 기기 enroll 은 trial 을 늘리지 않음) |
| trial 리마인더 | `mail_log` | `{user_id}:trial_ending_soft:D23` — due-scan 중복 발송 차단 |
| 영수증 | `payments` + `mail_log` | provider event id 기준. 동일 `invoice_id` 재수신 시 상태만 갱신, 메일은 `mail_log` 로 1회 |
| dunning | `dunning` + `mail_log` | `attempt` 별 1회(`payment_failed_d0/d3/d7`) |
| 취소 | `subscriptions` | `cancel_at_period_end` 멱등(두 번 취소해도 `access_until` 동일) |
| 머신 한도 | `machines`(`store.rs:72`) | 카운트는 `owner_user_id` 기준; 초과 enroll 은 거부하고 `machine_limit_reached` 는 `mail_log` 로 1회 |

동시성: 모든 갱신은 flock + 원자적 rename(`store.rs:195`, `store.rs:216`)을 타는 `mutate`(`service.rs:193-204`) 한 경로로만 수행한다. relay ticker 처럼 async 런타임에서 store 파일 I/O 를 할 때는 기존 규약대로 `run_blocking`(`src-tauri/src/ipc/mod.rs:35`)으로 감싼다.

#### 5. 결정 (Decisions)

##### D1. 체험 시작 트리거 — 가입 vs 첫 머신 enroll
- 옵션 A: `login_consume` 시점(가입). 옵션 B: **첫 `/machines/enroll` 성공 시점**.
- 권고: **B**.
- 근거: 사용자 레코드는 로그인만으로도(`service.rs:648-656`), 그리고 디바이스 승인만으로도(`service.rs:538`) 생성된다. A 를 택하면 제품을 한 번도 쓰지 않은 계정이 30일을 소모하고, 같은 사람의 두 번째 기기에서 체험을 다시 시작할지 말지가 애매해진다. B 는 체험이 **실제 사용 시작(= grant 를 받을 수 있는 상태, `service.rs:913`)과 정렬**되고, `trials` 키를 `user_id` 로 두면 두 번째 기기에서 늘어나지 않는다. 예외: 이미 `subscriptions.status = active` 인 계정은 trial 을 만들지 않는다.

##### D2. 체험 리마인더 스케줄러 — relay 내부 ticker vs 외부 cron
- 옵션 A: **relay 내부 ticker**. 옵션 B: 외부 cron/잡이 관리 엔드포인트를 두드림.
- 권고: **A**. 단, 판정 로직은 순수 함수로 분리한다: `due_letters(&AccountStore, now) -> Vec<(user_id, MailLetter, MailVars)>` 는 store 만 입력으로 받는다.
- 근거: relay 는 이미 백그라운드 주기 작업을 스폰한다(`bin/relay.rs:124` 의 `spawn_session_reaper`) 그리고 `tokio::time::interval` 패턴이 코드베이스에 이미 있다(`remote/relay_client.rs:527`, `remote/server.rs:3717`). B 는 상태 조회·중복 방지 로직을 두 곳으로 쪼개고, 외부 잡에 계정 store 접근 권한을 새로 열어야 한다. A 의 재기동 누락 위험은 "due-scan 을 store 에서 매번 재계산" 규칙으로 제거한다(메모리 상태 없음).

##### D3. 발신 도메인/아이덴티티 — 단일 `login@checka.cc` 유지 vs 결제용 서브도메인 분리
- 옵션 A: 현행 단일 주소(`mailer.rs:250-251`). 옵션 B: **결제/체험 메일을 별도 서브도메인·From 으로 분리**.
- 권고: **B**, 코드 변경은 `FERRYX_MAIL_FROM` 값 교체 1건(`mailer.rs:250`)으로 끝낸다.
- 근거: 지금은 매직링크·디바이스 승인·(신규) 영수증·독촉이 한 평판을 공유한다. 거래성 메일의 발신 평판이 마케팅/알림과 섞이면 영수증이 스팸으로 밀릴 수 있고, DKIM 셀렉터·SPF 를 분리하면 장애 격리가 된다.

##### D4. 체험 종료 직후 처리 — 즉시 차단 vs 유예(grace)
- 옵션 A: `ends_at` 즉시 paywall. 옵션 B: **3일 읽기 전용 유예 후 paywall**.
- 권고: **B**. 근거: `trial_expired` 메일과 dunning 파이프라인(`payment_failed_d0`)이 같은 재시도/로그 인프라를 쓰므로, 유예 기간이 "메일 발송 실패 → 재시도"와 "사용자 결제 지연"을 모두 흡수한다. 차단 시점은 `trial_ends_at + 3d` 로 `trials` 레코드에서 계산 가능하다.

#### 6. 단계 계획 (Phase plan)

- **P0 — 템플릿 계층 (신규)**: `src-tauri/src/account/mail_templates.rs` 에 `enum MailLetter`(§1 의 13개 variant) + `MailVars` + `render(letter, locale, vars) -> RenderedMail { subject, html, text }`, 본문은 `src-tauri/src/account/templates/*.html` 을 `include_str!` 로 로드. `mailer.rs:30-31` 의 `Mailer` 트레이트에 `fn send(&self, letter: MailLetter, to: &str, vars: &MailVars)` 를 추가하고 `send_magic_link` 은 `MailLetter::LoginMagicLink` 위임 래퍼로 유지한다(기존 호출부 `service.rs:445-447`, `service.rs:604` 무변경).
- **P1 — store 확장 (신규)**: `store.rs:132-147` 에 §4 의 6개 map 추가, 상수 `TRIAL_LENGTH: Duration = 30 * 24h` · `MAIL_RETRY_LIMIT: u32 = 3` 을 `store.rs:10-14` 옆에 배치, `purge_expired`(`store.rs:166-172`)에 만료 `mail_log`/`dunning` 정리 추가.
- **P2 — 라우트 (신규)**: `service.rs:1111-1138` 라우터에 `POST /api/account/v1/billing/webhook`(`billing_webhook`), `GET /api/account/v1/subscription`(`get_subscription`, `require_user`), `POST /api/account/v1/subscription/checkout`(`create_checkout`), `POST /api/account/v1/subscription/cancel`(`cancel_subscription`), `POST /api/account/v1/mail/events`(`mail_events`) 추가. trial 시작은 `enroll`(`service.rs:1131` 라우트)의 **같은 `mutate` 클로저 안에서** `trials` 를 쓰고 메일 큐 항목을 남긴다(원자성).
- **P3 — 라이프사이클 (신규)**: `src-tauri/src/account/lifecycle.rs` 에 순수 함수 `due_letters(&AccountStore, now)` + `spawn_lifecycle_ticker(state)` 를 만들고 `bin/relay.rs:124` 옆에서 스폰한다. store I/O 는 `run_blocking`(`src-tauri/src/ipc/mod.rs:35`) 경유, tick 주기는 1시간(테스트는 `now` 주입).
- **P4 — 페이월/한도 (신규)**: `subscriptions.status != active && trial 종료(+grace)` 일 때 relay attach/grant 경로에서 402 `PAYWALL_REQUIRED`, `enroll` 에서 `machines` 카운트 검사 후 `MACHINE_LIMIT_REACHED` + `machine_limit_reached` 메일.

##### 수용 테스트 (명령 + 기대 관측값)
1. 템플릿 완전성: `cargo test --manifest-path src-tauri/Cargo.toml --lib account::mail_templates`
   → 전 variant 렌더 성공, KO/EN 제목 비어있지 않음, `required_vars()` 누락 시 `render` 실패. 기대: `test result: ok. N passed; 0 failed`.
2. 리마인더 1회성(결정적 시계 주입): `cargo test --manifest-path src-tauri/Cargo.toml --lib account::lifecycle`
   → D+22 없음, D+23 `TrialEndingSoft`, D+29 `TrialEndingFinal`, D+30 `TrialExpired`; `mail_log` 기록 후 같은 `now` 재호출 시 빈 vec.
3. store 마이그레이션: `cargo test --manifest-path src-tauri/Cargo.toml --lib account::store`
   → 구버전 store JSON 로드 성공(`#[serde(default)]`), 만료 레코드 purge, login_codes 는 purge 제외 유지(`store.rs:167-171`).
4. 매직링크 실발송(로컬): `FERRYX_ACCOUNT_ORIGIN=http://127.0.0.1:43822 FERRYX_ACCOUNT_DATA_DIR=$(mktemp -d) FERRYX_MAIL_DIR=$(mktemp -d) cargo run --manifest-path src-tauri/Cargo.toml --bin ferryx-account` 실행 후
   `curl -s -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:43822/api/account/v1/login/request -H 'content-type: application/json' -d '{"email":"e2e@example.com"}'`
   → `202`, `FERRYX_MAIL_DIR` 파일 수 `+1`.
5. 요청 제한: 같은 주소로 6회 반복 → 6번째 `429`(`LOGIN_RATE_LIMITED`, `service.rs:595-599`), 다른 주소는 `202`.
6. 전송 실패 경로: `FERRYX_MAIL_DIR` 미설정으로 4번을 실행 → `503`(`MAIL_FAILED`, `service.rs:606-610`, `mailer.rs:54`).
7. 웹훅 멱등성(신규): `curl -s -o /dev/null -w '%{http_code}' -X POST https://relay.checka.cc/api/account/v1/billing/webhook -H 'content-type: application/json' -d '{"id":"evt_test_1","type":"invoice.paid","invoice_id":"in_test_1","user_id":"u_test"}'` 를 **두 번** 실행 → 둘 다 `200`, 메일 파일 수는 1회만 증가.
8. 기존 회귀 앵커 유지: `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::device_flow_lifecycle_request_poll_approve`(`service.rs:1173-1174`) → 계속 통과.

</details>

### 3.9 데스크톱 앱 (Tauri: macOS/Windows/Linux)
- 라이선스 로컬 보관·오프라인 검증·로컬 기능 무게이트 → 상세: `01-ls-core.md`(검증) + `05-client-ux.md`(표시)
- 핵심 계약(요약): 앱은 **계정 없이도 완전 동작**하며, 라이선스는 원격 기능 사용 시에만 필요하다. 데몬은 라이선스 없이 기동한다(셀프호스트 보호).

### 3.10 프런트엔드 UX (데스크톱 UI + 원격 웹)
- 상태 표면·뱃지·업그레이드 경로·문자열 → 상세: `05-client-ux.md`


<details>
<summary>레인 상세 원문 (05-client-ux.md)</summary>

# 05. 클라이언트 UX — 데스크톱 앱 / 원격 웹 / 모바일

> 범위: 사용자가 **어디서** 플랜·체험 잔여일·기기 수를 보고, **어떻게** 결제로 이동하고, **무엇이** 실제로 막히는지만 다룬다.
> 요금/발급/서버 모델은 01–04 섹션 소관이다. 이 문서는 클라이언트가 소비할 필드 이름만 가정하고 명시한다(§10).
> 표기 규칙: `현재`는 이 세션에서 파일을 읽고 확인한 사실이며 file:line을 붙였다. `신규`는 이 문서가 제안하는 작업이다.

#### 0. 확인된 현재 구조

| # | 현재 사실 | 근거 |
|---|---|---|
| C1 | 데스크톱/원격 분기는 `__TAURI_INTERNALS__` 존재 여부로 갈리고, 데스크톱은 계정과 무관하게 `App`을 부팅한다. | `ui/src/main.tsx:19`, `ui/src/main.tsx:50` |
| C2 | 원격 클라이언트는 **attach 경로에서만** 계정을 요구한다: 토큰이 없으면 로그인/머신 목록 화면을 렌더한다. | `ui/src/remote/RemoteApp.tsx:1148`, `ui/src/remote/RemoteApp.tsx:1161` |
| C3 | 계정 API origin은 페이지 origin이 아니라 릴레이 origin이다(데스크톱 내장 서버는 account 라우터를 마운트하지 않음). 해석은 `resolveAccountOrigin` + health probe + sessionStorage 캐시. | `ui/src/remote/accountSession.ts:171`, `ui/src/remote/accountSession.ts:197` |
| C4 | 제품 기본 릴레이는 단일 소스 오브 트루스에서 온다. | `ui/src/lib/pairedHostInventory.ts:8`, `ui/src/remote/accountSession.ts:171` |
| C5 | 데스크톱 설정에서 계정 로그인은 `Remote` 섹션 **안쪽**에만 존재한다(설정 전체를 계정으로 잠그지 않음). | `ui/src/components/settings/RemoteSection.tsx:596` |
| C6 | 데스크톱 설정은 고정 섹션 레지스트리 + 좌측 nav + 조건부 렌더로 구성된다. | `ui/src/components/SettingsDialog.tsx:50`, `ui/src/components/SettingsDialog.tsx:145`, `ui/src/components/SettingsDialog.tsx:162`, `ui/src/components/settings/types.ts:1` |
| C7 | 원격 헤더는 연결 배지·호스트 스위처·attention 배지를 한 클러스터에 담고, 좁은 폭에서 라벨을 접는다. | `ui/src/remote/RemoteApp.tsx:1219`, `ui/src/remote/RemoteApp.tsx:1245` |
| C8 | 모바일용 호스트 전환은 bottom-sheet 드로어로 구현돼 있다. | `ui/src/remote/MobileHostDrawer.tsx:141` |
| C9 | 모바일 챗 워크스페이스가 원격 셸에 이미 붙어 있다. | `ui/src/remote/RemoteApp.tsx:34` |
| C10 | 기기 수를 보여줄 표면은 이미 있다: 설정 `Remote`의 인가된 기기 목록. | `ui/src/components/settings/RemoteAccessSection.tsx:31`, `ui/src/components/settings/RemoteAccessSection.tsx:47` |
| C11 | 클라이언트 캐시 선례 2개: 호스트 인벤토리는 `ferryx_remote_hosts`로 영속, origin probe는 sessionStorage에 1회 캐시. | `ui/src/state/remoteHostStore.ts:35`, `ui/src/remote/accountSession.ts:173` |
| C12 | 마이그레이션되는 저장 키 헬퍼가 있다(신규 라이선스 캐시도 이 경로를 쓴다). | `ui/src/lib/storageKeys.ts:34`, `ui/src/lib/storageKeys.ts:67` |
| C13 | **라이선스/체험 개념이 코드에 없다**: `license` 식별자와 `"trial"` 문자열이 UI·백엔드 소스 전체에서 0건이다(ast-grep 검색). | 검색 결과 0건 (`ui/src/**`, `src-tauri/src/**`) |
| C14 | 설정 UI 프리미티브는 이미 있다(섹션 헤더/그룹/행). | `ui/src/components/settings/primitives.tsx:3`, `:23`, `:50` |
| C15 | 데스크톱 설정 상단 일반 섹션은 업데이터·CLI 런처 카드를 포함한다(플랜 카드가 들어갈 자리). | `ui/src/components/settings/GeneralSection.tsx:30`, `ui/src/components/settings/GeneralSection.tsx:34`, `ui/src/components/SettingsDialog.tsx:236` |
| C16 | 한국어 인라인 문자열 선례는 이미 있고 i18n 프레임워크는 없다. | `ui/src/components/settings/SshSection.tsx:1105`, `ui/package.json:16-61`(의존성 목록 전체) |

#### 1. 라이선스/엔타이틀먼트 상태 표면

##### 1.1 상태 모델 (클라이언트는 이 9개 상태만 렌더한다)

| state | 트리거(서버) | 데스크톱 설정 | 원격 헤더 | 모바일 | 로컬 터미널/워크트리 |
|---|---|---|---|---|---|
| `no_account_self_host` | 계정 토큰 없음 + 라이선스 없음 | "로컬 전용" 카드 | 배지 없음 | 배지 없음 | 정상 |
| `trial_active` | `trialDaysLeft > 3` | "체험 D-{n}" | 중립 칩 | 중립 칩 | 정상 |
| `trial_ending` | `trialDaysLeft <= 3` | amber 카드 + 갱신 CTA | amber 칩 | amber 칩 | 정상 |
| `trial_expired` | `state=trial_expired` | 종료 카드 + 플랜 목록 | amber 배너 | 배너 + 시트 | **정상** |
| `paid_active` | `state=paid_active` | 플랜명 + 기기 {used}/{limit} | 배지 없음(조용함) | 없음 | 정상 |
| `past_due` | `state=past_due`, `graceUntil` | amber + "결제 실패" | amber 칩 | amber 칩 | 정상(유예 기간) |
| `canceled_at_period_end` | `state=canceled`, `periodEnd` | "취소됨 · {date}까지" | 중립 칩 | 중립 칩 | 정상 |
| `entitlement_exceeded` | `used > limit` | rose 카드 + 기기 목록 | rose 칩 | 시트 상단 경고 | 정상(기존 기기 유지, 신규 등록만 차단) |
| `unknown`(오프라인/미인증) | 조회 실패 | 마지막 스냅샷 + 시각 | 회색 칩 | 회색 칩 | 정상 |

`trial_expired`의 "정상"은 계약이다: 체험 종료는 **원격/릴레이 기능만** 막고 로컬 사용은 막지 않는다(§3 근거).

##### 1.2 (a) 데스크톱 앱 설정

- 신규 `ui/src/components/settings/PlanSection.tsx` — C14 프리미티브(`SettingsHeading`/`SettingsGroup`/`SettingRow`)를 그대로 쓴다.
- 변경 `ui/src/components/SettingsDialog.tsx`: `VALID_SECTIONS`(:50)에 `"plan"` 추가, 좌측 nav(:145)에 `NavButton` 추가, 본문 스위치(:162) 옆에 `{section === "plan" ? <PlanSection /> : null}` 추가. `SectionId` 유니온은 `ui/src/components/settings/types.ts:1`에 `"plan"`을 추가한다.
- 신규 `ui/src/components/settings/LicenseCard.tsx` — 플랜명·상태·기기 수·CTA. 기기 수는 C10의 기존 카운트를 재사용 가능(현재는 `listRemoteDevices()` 호출: `RemoteAccessSection.tsx:47`).
- 신규 `ui/src/components/settings/LicensePasteField.tsx` — 셀프호스트 키 입력(§4).
- `no_account_self_host`에서는 **계정 로그인을 유도하지 않는다**. 현재 계정 진입점은 `Remote` 섹션 내부뿐이므로(`RemoteSection.tsx:596`) 새 섹션은 그 조건을 바꾸지 않는다.

##### 1.3 (b) 원격 웹 클라이언트 헤더

- 신규 `ui/src/remote/LicenseBanner.tsx` + `ui/src/remote/useLicenseState.ts`, 마운트 지점은 C7 클러스터 — `RemoteApp.tsx:1219` 연결 배지 **앞**, `RemoteApp.tsx:1245` 호스트 스위처와 같은 `flex shrink-0` 행.
- 표시 규칙: `trial_active`는 칩 + 클릭 시 상세, `paid_active`/`no_account_self_host`는 **렌더링하지 않는다**(원격 화면을 광고판으로 만들지 않는다).
- 데스크톱이 내장 서버로 원격 클라이언트를 서빙하는 경우(계정 라우터 미마운트, C3)에는 계정 기반 배지를 띄우지 않는다 — origin 해석이 페이지 origin을 신뢰하지 않기 때문(`accountSession.ts:197`).

##### 1.4 (c) 모바일(폰 폭)

- 같은 헤더 클러스터가 폭을 줄여 라벨을 숨기므로(`RemoteApp.tsx:1219`~`:1245`) 모바일에서는 **아이콘 칩 1개**만 노출하고, 탭하면 시트를 연다.
- 신규 `ui/src/remote/LicenseSheet.tsx` — C8 드로어와 동일한 구조(`fixed inset-0 z-50 flex flex-col justify-end` + `role="dialog"`)를 복제해 일관성을 유지한다(`MobileHostDrawer.tsx:141`).
- 시트 내용: 상태 한 줄 · 잔여일/기한 · 기기 `{used}/{limit}` · 기본 CTA(업그레이드/결제 관리) · 보조 CTA(상태 새로고침).

#### 2. 업그레이드/페이월 UX

##### 2.1 클릭 경로

| 표면 | 트리거 | 화면 | CTA | 목적지 |
|---|---|---|---|---|
| 데스크톱 | 설정 → Plan | `PlanSection` + `LicenseCard` | "업그레이드" | 시스템 브라우저로 `checkoutUrl` |
| 데스크톱(원격 섹션) | Remote 토글 ON 시도 | 인라인 안내 | "업그레이드" | 동일 |
| 원격 웹 | 헤더 칩 | 드롭다운 패널 | "업그레이드" | 같은 탭 네비게이션 |
| 모바일 | 헤더 칩 | `LicenseSheet` | "업그레이드" | 같은 탭 네비게이션 |
| 결제 후 | 복귀 URL | 상태 재조회 | — | `#plan=...` 처리(§4.3) |

- 데스크톱은 인앱 웹뷰보다 **시스템 브라우저**를 기본으로 한다. 이미 외부 열기 명령이 명령 레지스트리에 등록돼 있다(`src-tauri/src/lib.rs:1393`의 `tauri::generate_handler![...]`, `cmd_browser_open_external` 포함).
- 원격/모바일은 이미 브라우저이므로 같은 탭 이동이면 충분하다.

##### 2.2 게이트 정책: 배지 우선, 하드 게이트는 원격/릴레이만

- **하드 게이트 대상**(서버 강제 + 클라 측 사전 안내): 릴레이 attach, 기기 등록, 기기 수 한도 초과 시 신규 등록.
- **배지만**: 플랜 표시, 기기 수 표시, 릴레이 URL 입력, 로컬 기능 전체.
- **절대 차단 금지**: 터미널 스폰/워크트리/에이전트 실행. 근거: 현재 데스크톱은 계정 없이 부팅하고(`main.tsx:19`, `main.tsx:50`), 원격 계정 게이트는 attach 경로 안쪽에만 있다(`RemoteApp.tsx:1148`). 신규 라이선스 로직이 이 경계를 넘으면 회귀다.
- `trial_expired` 상태에서도 **이미 열린 터널/세션은 끊지 않는다**. 배너는 비차단(`role="status"`, `aria-live="polite"`)이며 새 attach 시도에서만 차단 사유를 보여준다.

#### 3. 셀프호스트 UX

- 모드 3가지: (1) 라이선스 없음 = 로컬 전용 무기한, (2) 영구 라이선스 키 보유 = `self_host_perpetual`, (3) 릴레이 구독 = 계정 기반.
- **모드 (1)·(2)에서 계정 프롬프트는 0회**여야 한다. 현재도 계정 진입점은 `Remote` 섹션 내부뿐이므로(`RemoteSection.tsx:596`) Plan 섹션에 "로그인" 유도 카드를 만들지 않는다.
- 키 입력 위치: 설정 → Plan → `license-paste-input` + "라이선스 키 붙여넣기" 버튼. 검증 실패는 `role="alert"`로 사유(서명 불일치/기기 한도/만료 형식)를 표시한다.
- 저장 위치(권고): 클라이언트는 마지막 확인 스냅샷만 `ferryx.license.snapshot` 키로 캐시하고(`getMigratedItem` 경유, `storageKeys.ts:34`), 권위 저장은 데몬 데이터 디렉터리다(`FERRYX_DATA_DIR` 해석: `src-tauri/src/daemon/server.rs:277`).
- 헤드리스/CLI 경로: `ferryx-cli license activate --key <key>` (CLI도 동일 데이터 디렉터리를 쓴다: `src-tauri/src/cli.rs:603`). GUI 없이 서버에 붙여야 하는 사용자가 여기 해당한다.
- 키 없는 자가호스터를 위한 안내는 **정보성**이다: "이 기기는 셀프 호스트로 동작합니다. 계정이 필요하지 않습니다." + 릴레이 구독을 원할 때만 링크.
- 체험/구독 상태는 릴레이 기능을 켤 때만 의미가 있다: 현재 릴레이 활성 토글은 `RemoteAccessSection.tsx:31`이 담당하므로, 이 토글 옆 배지 1줄로 제한한다.

#### 4. 모바일 세부

##### 4.1 체험 종료 화면

- 헤더 칩(amber) → `LicenseSheet` 상단: 제목 "체험판이 종료되었습니다", 본문에서 **로컬은 계속 동작**한다는 사실을 먼저 말하고, 원격 재개를 위한 플랜 선택을 제시한다.
- 시트에는 닫기 버튼과 "지금은 하지 않기"를 둔다(모달 강제 금지). 원격 attach 실패 화면에서만 하드 블록을 보여준다.

##### 4.2 기존 세션 유지

- 이미 연결된 세션 화면은 유지한다. 상태 변화는 배너로만 반영하고, `RemoteTerminal`을 언마운트하지 않는다(현재 헤더는 배지/스위처/attention만 관리: `RemoteApp.tsx:1219`, `RemoteApp.tsx:1245`).

##### 4.3 결제 후 복귀(딥링크)

- 현재 로그인 복귀는 hash/query 파싱으로 처리한다: `#code=`, `#login=`, `#account_token=` (`AccountLoginPage.tsx:26`, `AccountLoginPage.tsx:30`), 설정 로그인도 동일 (`AccountSignIn.tsx:38`).
- 신규: 같은 파서를 확장해 `#plan=success|past_due|canceled|exceeded`를 `licenseStore.applyCheckoutReturn()`이 소비하고, `history.replaceState`로 흔적을 지운 뒤 상태를 재조회한다(기존 정리 방식과 동일).
- 복귀 URL은 `checkoutUrl` 생성 시 `return=` 파라미터로 넘긴다. 계정이 없어 토큰을 못 받은 경우엔 "결제는 완료됐지만 이 기기에서 아직 확인되지 않았습니다 · 새로고침" 상태를 보여준다.

##### 4.4 오프라인

- 조회 실패 시 마지막 스냅샷 + "마지막 확인 {date}"를 표시한다(`unknown` 행). 이 상태는 **권한을 늘리지 않는다**(캐시는 degrade-only).

#### 5. 접근성 / i18n

- **i18n 프레임워크 없음**: `ui/package.json:16-61`의 dependencies/devDependencies 어디에도 i18n 패키지가 없고 문자열은 인라인이다. 한국어 인라인 선례도 이미 있다(`SshSection.tsx:1105`).
- 권고: 신규 `ui/src/lib/strings.ts`에 타입드 키 → `{ko, en}`을 두고 `t("key", params)`로 접근한다. 기본 로케일 `ko-KR`, 폴백 `en`. 기존 영어 인라인은 이번 작업 범위에서 건드리지 않는다(점진 이관).
- 접근성 규칙(신규 컴포넌트 공통): 상태 변화는 `role="status"` + `aria-live="polite"`, 오류는 `role="alert"`, 모든 인터랙티브 요소에 `aria-label` 또는 가시 라벨, 테스트 훅은 `data-testid`(기존 관례: `RemoteApp.tsx:1219`, `AccountMachinesPage.tsx:141`), 시트 포커스 트랩은 `AddMachineModal` 패턴(`ui/src/components/settings/AddMachineModal.tsx:154`) 재사용.
- 색만으로 상태를 구분하지 않는다: amber/rose 칩에는 항상 텍스트 라벨을 동반한다.

##### 5.1 문자열 표 (신규, `ui/src/lib/strings.ts`)

| key | ko | en |
|---|---|---|
| `license.badge.trialActive` | 체험 {days}일 남음 | {days} days left in trial |
| `license.badge.trialEnding` | 체험 {days}일 남음 · 곧 종료 | Trial ends in {days} days |
| `license.badge.trialExpired` | 체험 종료 | Trial ended |
| `license.badge.paidActive` | {plan} 사용 중 | {plan} active |
| `license.badge.pastDue` | 결제 실패 · {date}까지 사용 가능 | Payment failed · usable until {date} |
| `license.badge.canceled` | 취소됨 · {date}까지 사용 가능 | Canceled · usable until {date} |
| `license.badge.selfHost` | 셀프 호스트 · 무기한 | Self-host · perpetual |
| `license.badge.localOnly` | 로컬 전용 | Local only |
| `license.badge.exceeded` | 기기 한도 초과 ({used}/{limit}) | Device limit reached ({used}/{limit}) |
| `license.action.upgrade` | 업그레이드 | Upgrade |
| `license.action.manageBilling` | 결제 관리 | Manage billing |
| `license.action.pasteKey` | 라이선스 키 붙여넣기 | Paste license key |
| `license.action.recheck` | 상태 새로고침 | Refresh status |
| `license.sheet.expiredTitle` | 체험판이 종료되었습니다 | Your trial has ended |
| `license.sheet.expiredBody` | 로컬 터미널과 워크트리는 계속 사용할 수 있습니다. 원격 접속을 계속하려면 요금제를 선택하세요. | Local terminals and worktrees keep working. Choose a plan to keep remote access. |
| `license.selfHost.body` | 이 기기는 셀프 호스트로 동작합니다. 계정이 필요하지 않습니다. | This machine runs self-hosted. No account is required. |
| `license.checkout.returning` | 결제가 완료되었습니다. 상태를 확인하는 중… | Payment received. Checking your status… |
| `license.offline.stale` | 오프라인 · 마지막 확인 {date} | Offline · last checked {date} |

#### 6. 결정 (Decisions)

##### D1. 배지 vs 하드 게이트 위치

- **옵션 A**: 모든 원격 기능 하드 게이트. 구현이 단순하지만 체험 종료가 로컬 UX를 오염시키고, 과거 원격 계열 회귀(워크트리 클릭 무반응)와 같은 실패 형태를 만든다.
- **옵션 B(권고)**: 배지 우선 + 하드 게이트는 릴레이 attach/기기 등록만(서버 강제, 클라이언트는 사전 안내).
- **옵션 C**: 게이트 없음(순수 유도). 전환율은 낮겠지만 릴레이 비용을 회수할 수 없다.
- **근거**: 현재 코드 경계가 이미 B와 같다 — 데스크톱은 계정 없이 부팅(`main.tsx:19`, `main.tsx:50`), 계정 게이트는 원격 attach 경로에만 존재(`RemoteApp.tsx:1148`). B는 그 경계를 유지하므로 로컬 회귀 위험이 가장 낮다.

##### D2. 라이선스 상태 캐시 위치와 오프라인 리프레시

- **옵션 A**: 서버 응답만 사용. 오프라인에서 상태 표시가 사라지고, 잘못 구현하면 로컬 사용까지 막는 유혹이 생긴다.
- **옵션 B(권고)**: 서명된 스냅샷을 클라이언트에 캐시(`ferryx.license.snapshot`, origin별). TTL 24h, 표시용 오프라인 grace 14일, **캐시는 권한을 늘릴 수 없다**(degrade-only). 강제는 서버.
- **옵션 C**: 로컬 카운터 기반 오프라인 유예(예: 30일). 시계 조작에 취약하고 서버 모델과 이중 진실을 만든다.
- **근거**: 캐시 선례가 둘 이미 있다 — 호스트 인벤토리 영속(`remoteHostStore.ts:35`)과 origin probe의 sessionStorage 1회 캐시(`accountSession.ts:173`, `:197`). 저장 키는 기존 헬퍼 경로를 쓴다(`storageKeys.ts:34`).

##### D3. 체크아웃 표면

- **옵션 A**: 인앱 웹뷰(데스크톱).
- **옵션 B(권고)**: 데스크톱은 시스템 브라우저, 원격/모바일은 같은 탭.
- **근거**: 데스크톱에는 이미 외부 열기 명령이 등록돼 있고(`src-tauri/src/lib.rs:1393`), 스토어 정책·결제 SDK 제약을 클라이언트가 떠안지 않는다. 원격 클라이언트는 그 자체가 브라우저라 추가 표면이 필요 없다.

#### 7. 단계 계획

| 단계 | 산출물 | 파일 |
|---|---|---|
| P1 상태/클라이언트 | 라이선스 상태 모델, 조회·캐시·오프라인 규칙 | 신규 `ui/src/state/licenseStore.ts`, `ui/src/lib/licenseClient.ts`, `ui/src/lib/licenseStorage.ts`, `ui/src/lib/strings.ts` |
| P2 데스크톱 설정 | Plan 섹션 등록 + 카드/붙여넣기 | 신규 `ui/src/components/settings/PlanSection.tsx`, `LicenseCard.tsx`, `LicensePasteField.tsx`; 변경 `ui/src/components/SettingsDialog.tsx:50`, `:145`, `:162`, `ui/src/components/settings/types.ts:1` |
| P3 원격/모바일 | 헤더 칩 + 시트 + 복귀 처리 | 신규 `ui/src/remote/LicenseBanner.tsx`, `ui/src/remote/LicenseSheet.tsx`, `ui/src/remote/useLicenseState.ts`; 변경 `ui/src/remote/RemoteApp.tsx:1219` 클러스터 |
| P4 셀프호스트/CLI | 키 활성화 + 헤드리스 경로 + 배지 | 신규 `ui/src/components/settings/LicensePasteField.tsx`(P2와 공유), 백엔드 `license` 모듈(01–04 의존), 명령 등록 `src-tauri/src/lib.rs:1393` |

각 단계는 앞 단계의 스토어 셀렉터만 소비하고, 게이트 판정은 `licenseStore`의 순수 함수(`isRemoteAttachAllowed()` / `canOpenLocalTerminal()` — 후자는 항상 `true`)로만 한다.

#### 8. 인수 테스트

**T1 — 상태 파생(경계 포함)**
- 명령: `bun run --cwd ui test src/state/licenseStore.test.ts`
- 판정 단언: `selectLicenseBadge({state:"trial_active",trialDaysLeft:2})` → `{tone:"warning",daysLeft:2}`, `trialDaysLeft:4` → `{tone:"neutral"}`, 토큰 없음 + 라이선스 없음 → `{kind:"local_only"}`.
- 기대 관측: 해당 파일 전부 pass, exit 0.
- 절차: 먼저 경계(`<=3`)를 `<=0`으로 뮤테이션해 **RED**를 확인한 뒤 되돌려 GREEN.

**T2 — 셀프호스트에 계정 프롬프트가 없다**
- 명령: `bun run --cwd ui test src/components/settings/PlanSection.test.tsx`
- 판정 단언: 키 입력 후 `activateLicense` 1회 호출, `getByText("셀프 호스트 · 무기한")` 렌더, 그리고 `expect(queryByTestId("account-sign-in")).toBeNull()`.
- 기대 관측: pass, exit 0. 현재 계정 진입점이 `RemoteSection.tsx:596` 하나뿐이라는 사실을 테스트가 고정한다.

**T3 — 체험 종료가 원격 표면만 막는다**
- 명령: `bun run --cwd ui test src/remote/RemoteUI.test.tsx -t "trial expired"`
- 판정 단언: `getByTestId("license-banner")`의 `data-state === "trial_expired"`이고, 동시에 `getByTestId("remote-connection-badge")`가 여전히 렌더된다(클라이언트 배지는 정보성이지 차단이 아님). 하드 블록은 새 attach 시도에서만 나타나므로 `queryByTestId("account-login-page")`는 `null`.
- 기대 관측: `-t` 필터로 해당 케이스만 실행, pass, exit 0. 기존 헤더 단언(`RemoteUI.test.tsx:855`)이 계속 통과해야 한다.

**T4 — 오프라인 캐시는 권한을 늘리지 않는다**
- 명령: `bun run --cwd ui test src/state/licenseStore.test.ts -t "offline"`
- 판정 단언: TTL(24h) 경과 + grace(14일) 이내 스냅샷 → `badge.stale === true`이고 `isRemoteAttachAllowed()`는 캐시된 값을 그대로 반환; `canOpenLocalTerminal()`은 스냅샷이 `trial_expired`여도 `true`.
- 기대 관측: pass, exit 0. `canOpenLocalTerminal()`을 `return license.state !== "trial_expired"`로 뮤테이션하면 **RED**여야 한다(이 단언이 로컬 차단 회귀를 잡는 지점).

테스트 실행기는 저장소 표준을 그대로 쓴다: `ui/package.json:12`의 `vitest run --maxWorkers=1`, jsdom + `ui/src/test/setup.ts`(`ui/vitest.config.ts:13`, `:14`). 모의는 기존 관례를 따른다 — 모듈 스파이(`AccountLoginPage.test.tsx:5`, `:24`) 또는 섹션 테스트의 IPC 스텁 하니스(`PermissionsSection.test.tsx:131`).

#### 9. 접근성/문구 검수(수동)

- 키보드만으로: 설정 → Plan → 키 붙여넣기 → CTA → 시스템 브라우저 열기까지 도달 가능한지.
- 스크린리더: 배지 변화가 `aria-live`로 1회만 읽히는지(중복 announce 금지).
- 폰 360px/390px에서 시트가 화면을 넘지 않는지, 하단 안전영역을 침범하지 않는지(`MobileHostDrawer.tsx:141` 레이아웃 계승 확인).

#### 10. 가정과 의존(명시)

- 서버 계약은 01–04 섹션이 정의한다. 클라이언트가 소비하는 필드명 가정: `{ state, plan, trialDaysLeft, periodEnd, graceUntil, seatsUsed, seatsLimit, checkoutUrl, checkedAt }`.
- 데스크톱 셀프호스트 활성화는 릴레이 서버가 아니라 **데몬 데이터 디렉터리**에 저장된다(`FERRYX_DATA_DIR`: `src-tauri/src/daemon/server.rs:277`). 서버 측 검증이 없다면 이 경로는 로컬 명예 시스템이며, 그 사실을 UI 문구로 숨기지 않는다.
- 이 문서의 신규 파일/라인 참조는 아직 존재하지 않는 제안이며, `현재` 표기 항목만 검증된 사실이다.

</details>

### 3.11 모바일
- 폰 폭 UI, 체험 만료 화면, 결제 복귀 처리 → 상세: `05-client-ux.md`

### 3.12 운영 / 배포 / 관측성
- 배포(로컬 전용 릴리스 정책 유지), 시크릿 관리, 메트릭/로그 → 상세: `06-ops-data-security.md`


<details>
<summary>레인 상세 원문 (06-ops-data-security.md)</summary>

# 06. 운영 · 데이터 저장소 이전 · 보안/어뷰징

> 범위: 계정/라이선스 상용화에 필요한 운영 절차, JSON→SQLite 이전, 시크릿 관리, 관측가능성, 어뷰징 대응, 셀프호스팅.
> 표기 규칙: 코드 위치는 2026-09-25 워킹트리 기준 `file:line` 실측이다. 아직 없는 기능은 `(신규)`로 표시한다.
> 프로덕션 규모: 2026-09-25 프로브 기준 `account-store.json`에 users=6, machines=3. 스토어 기본 경로는 `~/.ferryx/account-data`(`src-tauri/src/bin/relay.rs:92`, `src-tauri/src/bin/relay.rs:133`).

## 0. 현재 상태 요약

- 계정 데이터는 **단일 JSON 파일**이다. `AccountStore`는 7개 `BTreeMap`(users, sessions, login_codes, enrollment_codes, machines, grants, device_auths)으로 구성된다(`src-tauri/src/account/store.rs:132`). 레코드 구조체는 `UserRecord`(`store.rs:42`)부터 `DeviceAuthRecord`(`store.rs:103`)까지다.
- 파일 경로는 `account-store.json`(`store.rs:187`), 쓰기는 0600 임시 파일 → `sync_all` → `rename` 원자 교체다(`store.rs:216`). 서명 키 파일은 같은 디렉터리의 `account-signing-key.json`(`store.rs:112`)이다.
- 동시성은 프로세스 간 advisory flock 하나로 직렬화된다: `lock_account_dir`가 디렉터리를 0700으로 만들고 `account-store.lock`에 `file.lock()`을 건다(`store.rs:195`). 모든 쓰기는 `mutate()`에서 락 → load → `purge_expired` → 변경 → save 순서로만 일어난다(`src-tauri/src/account/service.rs:193`).
- TTL/레이트리밋은 상수다: 로그인 코드 600초·세션 30일·인롤먼트 코드 600초(`store.rs:10`~`store.rs:12`), 이메일당 시간당 로그인 5회(`store.rs:13`), 본문 4096바이트(`store.rs:14`), 그랜트 TTL 600초(`service.rs:30`).
- 토큰은 평문 저장이 아니라 SHA-256 해시로 저장된다(`store.rs:23`; 세션 조회 `store.rs:178`). 무작위 토큰은 OsRng 32바이트 hex다(`store.rs:30`).
- 릴레이는 **단일 바이너리**로 계정 API와 터널 스플라이스를 함께 서빙한다: 라우터 조립 `src-tauri/src/remote/relay_server.rs:2167`, 계정 라우터 병합 `relay_server.rs:2233`, 기동 `src-tauri/src/bin/relay.rs:153`. 트레이싱 초기화는 `bin/relay.rs:105`다.

## 1. 저장소 이전: JSON 파일 → SQLite

### 1.1 구조적 한계(스케일이 아니라 정합성 문제)

1. **쓰기 전체 재작성**: 모든 변경이 파일 전체를 다시 쓴다(`service.rs:193` → `store.rs:161` → `store.rs:216`). 단일 프로세스에서는 flock으로 안전하지만, 결제 웹훅 수신기·백필 잡·운영 CLI가 동시에 쓰는 순간 read-modify-write 전체 파일 교체는 lost-update 위험이 생긴다.
2. **제약 표현 불가**: JSON `BTreeMap`에는 UNIQUE/ FK 가 없다(`store.rs:132`). 결제 이벤트 멱등성(`provider_event_id` 중복 무시)과 라이선스 키 유일성은 문서 규율이 아니라 DB 제약이어야 한다.
3. **조회 비용**: 이메일→유저는 선형 스캔이다(`store.rs:174`). 6 users에서는 무의미하지만, 결제/구독 집계(예: 트라이얼 전환율)를 파일 스캔으로 만들 수는 없다.
4. **감사 추적 부재**: `payment_events`/`subscriptions` 같은 append-only 성격의 이력이 들어오면 "최신 상태 1개"만 남기는 저장 모델과 충돌한다.

### 1.2 이전 트리거(하나라도 충족하면 착수)

- **T1 (권장 기본)**: `subscriptions`·`payment_events`·`license_keys` 중 하나라도 프로덕션에 쓰이기 시작하는 릴리스. 즉 **첫 유료 고객 이전**. 근거: 위 1.1-(2)는 규모가 아니라 correctness 요구다.
- **T2**: `account-store.json` 크기 > 1 MiB **또는** `mutate()` p95 > 50 ms(운영 메트릭 기준).
- **T3**: 계정 스토어를 쓰는 **별도 프로세스**가 2개 이상이 되는 배포(예: 릴레이 + 백그라운드 빌링 워커).
- **T4**: users > 100 또는 machines > 300.

T1 이전(현재 규모)에는 이전하지 않는다. 이전은 공짜가 아니고, 지금은 flock 한 줄(`store.rs:195`)이면 충분히 정확하다.

### 1.3 스키마 v1 (SQLite, `PRAGMA user_version = 1`)

```sql
PRAGMA journal_mode = WAL;      -- 단일 라이터 + 동시 읽기
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

-- store.rs:42 UserRecord
CREATE TABLE users (
  user_id TEXT PRIMARY KEY, email TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE UNIQUE INDEX users_email_unique ON users(email);          -- store.rs:174 대체

-- store.rs:50 SessionRecord (키 = token_hash, store.rs:23/178)
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL);
CREATE INDEX sessions_user_id ON sessions(user_id);
CREATE INDEX sessions_expires_at ON sessions(expires_at);         -- purge_expired 대체(store.rs:166)

-- store.rs:57 LoginCodeRecord
CREATE TABLE login_codes (
  code_hash TEXT PRIMARY KEY, email TEXT NOT NULL, expires_at INTEGER NOT NULL);
CREATE INDEX login_codes_expires_at ON login_codes(expires_at);

-- store.rs:64 EnrollmentCodeRecord
CREATE TABLE enrollment_codes (
  code_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  account_origin TEXT NOT NULL, expires_at INTEGER NOT NULL);

-- store.rs:72 MachineRecord (owner_user_id → user_id)
CREATE TABLE machines (
  machine_record_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  machine_id TEXT NOT NULL, display_name TEXT NOT NULL,
  public_key TEXT NOT NULL, attach_public_key TEXT NOT NULL,
  relay_origin TEXT NOT NULL, platform TEXT NOT NULL,
  enrollment_epoch INTEGER NOT NULL, enrolled_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX machines_machine_id_unique ON machines(machine_id);  -- service.rs:782 클레임 규칙
CREATE INDEX machines_user_id ON machines(user_id);                      -- list_machines(service.rs:710)

-- store.rs:89 GrantRecord (pairing_token_hash만 저장)
CREATE TABLE grants (
  grant_id TEXT PRIMARY KEY,
  machine_record_id TEXT NOT NULL REFERENCES machines(machine_record_id) ON DELETE CASCADE,
  owner_user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  pairing_token_hash TEXT NOT NULL, grant_scope TEXT NOT NULL,
  device_attach_public_key TEXT NOT NULL, installation_id TEXT NOT NULL,
  issued_at INTEGER NOT NULL, expires_at INTEGER NOT NULL);
CREATE INDEX grants_expires_at ON grants(expires_at);

-- store.rs:103 DeviceAuthRecord
CREATE TABLE device_auths (
  device_code_hash TEXT PRIMARY KEY, user_code TEXT NOT NULL, email TEXT NOT NULL,
  email_token_hash TEXT NOT NULL, enrollment_code TEXT, expires_at INTEGER NOT NULL);
CREATE INDEX device_auths_user_code ON device_auths(user_code);

-- (신규) 구독: 단일 개인 계정 = 계정당 1행
CREATE TABLE subscriptions (
  subscription_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE REFERENCES users(user_id) ON DELETE CASCADE,
  plan TEXT NOT NULL, status TEXT NOT NULL,
  trial_started_at INTEGER, trial_ends_at INTEGER, current_period_end INTEGER,
  cancel_at_period_end INTEGER NOT NULL DEFAULT 0,
  provider TEXT NOT NULL, provider_customer_id TEXT, provider_subscription_id TEXT,
  updated_at INTEGER NOT NULL);
CREATE INDEX subscriptions_status ON subscriptions(status);

-- (신규) 결제 이벤트: PCI 데이터 0, 멱등 키만
CREATE TABLE payment_events (
  event_id TEXT PRIMARY KEY, provider TEXT NOT NULL, provider_event_id TEXT NOT NULL,
  kind TEXT NOT NULL, user_id TEXT REFERENCES users(user_id),
  received_at INTEGER NOT NULL, payload_sha256 TEXT NOT NULL,
  processed_at INTEGER, status TEXT NOT NULL);
CREATE UNIQUE INDEX payment_events_provider_event_unique ON payment_events(provider, provider_event_id);
CREATE INDEX payment_events_user_id ON payment_events(user_id);

-- (신규) 라이선스 키: 서명 블롭 + 해시만 저장, 평문 키는 저장 금지
CREATE TABLE license_keys (
  license_key_hash TEXT PRIMARY KEY, license_key_id TEXT NOT NULL,
  user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
  plan TEXT NOT NULL, status TEXT NOT NULL, max_machines INTEGER NOT NULL,
  signature_b64 TEXT NOT NULL, issued_at INTEGER NOT NULL,
  expires_at INTEGER, revoked_at INTEGER);
CREATE UNIQUE INDEX license_keys_key_id_unique ON license_keys(license_key_id);
CREATE INDEX license_keys_user_id ON license_keys(user_id);
```

### 1.4 온라인 이전 절차(무중단, 롤백 가능)

1. **드라이런(운영 중 실행)**: `ferryx-account migrate --dry-run`(신규 서브커맨드, `src-tauri/src/bin/account.rs`)이 `lock_account_dir`(`store.rs:195`)를 잡고 JSON을 읽기만 해서 행 수·스키마 매핑·충돌 이메일을 보고한다. 파일은 건드리지 않는다.
2. **가드 플래그**: drop-in에 `FERRYX_ACCOUNT_STORE=json|sqlite`를 추가한다. 기본값은 전환 릴리스 동안 `json` 유지.
3. **적용**: `FERRYX_ACCOUNT_STORE=sqlite`로 재시작하면 릴레이 기동 시(`bin/relay.rs:153` 이전) `SqliteStore::open_or_import()`가 같은 락 아래에서 (i) `account-store.json.pre-sqlite.bak` 스냅샷, (ii) 단일 트랜잭션 import, (iii) 행 수 대조(users/machines/sessions/grants), (iv) `PRAGMA user_version=1` 기록을 수행한다. 6 users 규모에서 수 ms 작업이다.
4. **검증**: import 직후 서비스는 `MIGRATION_APPLIED users=… machines=… version=1`을 로그에 남기고, 실패하면 import 트랜잭션을 롤백하고 `json`으로 계속 서빙한다(fail-open이 아니라 "기존 경로 유지").
5. 계정 API 중단은 터널 세션 중단이 아니다: 스플라이스 경로(`relay_server.rs:2217`~`relay_server.rs:2220`의 `/tunnel/*` 라우트)는 계정 스토어를 읽지 않으므로, 이전 중에도 연결된 터미널은 유지된다. 잠깐 막히는 것은 로그인/인롤먼트/그랜트 발급뿐이다.

### 1.5 롤백

- 전환 릴리스 동안 `FERRYX_ACCOUNT_STORE=json`으로 되돌리면 즉시 이전 동작으로 복귀한다(코드 롤백 불필요).
- 되돌리면 sqlite 모드 기간의 쓰기(로그인 세션 등)는 사라진다. 따라서 **커트오버 시점은 쓰기량이 낮은 시각**(T1 시점, 런치 전)에 잡고, 커트오버 후 14일간 매일 밤 `account-store.json.export`(읽기 전용 스냅샷)를 남겨 롤백 손실 창을 하루로 묶는다.
- 커트오버 기록(`cutover_at`)을 DB에 남기고, 롤백 시 "export 이후 재적용할 쓰기 범위"를 로그로 확인한다.

### 1.6 백업/복구

- **일일 백업**: systemd timer(`ferryx-account-backup.timer`)가 `sqlite3 account.db ".backup '/var/lib/ferryx/backups/account-$(date +%F).db'"`를 실행한다(SQLite 온라인 백업 API, 서비스 재시작 불필요). 14일 보존 + 주 1회 `VACUUM INTO` 스냅샷.
- **리허설**: `scripts/relay/restore-drill.sh`가 최신 백업을 임시 디렉터리로 복원 → `PRAGMA integrity_check` → 행 수 대조 → `RESTORE_DRILL_PASS/FAIL` 출력. 분기마다 1회 실측한다.
- **복구 절차**: `systemctl stop ferryx-relay` → 현재 DB를 `.corrupt-<ts>`로 이동 → 백업 복사 → `integrity_check` 통과 시 `systemctl start ferryx-relay`. 터널은 기동 후 데몬 자동 재연결로 복귀한다(2026-09-25 라이브 실측 기록: `notes/ferryx-account-grants-audit-and-regressions-2026-09-25.md` §11).

### 1.7 전환 기간의 파일락 의미론

- 구·신 바이너리 모두 **같은 락 파일(`account-store.lock`, `store.rs:195`)** 을 먼저 잡는다. `mutate()`(`service.rs:193`)의 첫 줄이 그대로 유지되므로, 짧은 혼합 버전 창에서도 read-modify-write가 직렬화된다.
- SQLite 쪽은 락 안에서 `busy_timeout=5000` + WAL로 열고, 쓰기는 단일 트랜잭션으로 커밋한다. flock은 "프로세스 간 직렬화", WAL은 "리더 비차단"을 담당해 역할이 겹치지 않는다.
- 전환 릴리스의 규칙: sqlite 모드는 JSON을 **읽지 않고 쓰지도 않는다**(import 1회 제외). 양방향 dual-write는 만들지 않는다 — 두 저장소가 갈라지는 사고가 정확성 이득보다 크다.

## 2. 시크릿/키 관리

### 2.1 계정 서비스 Ed25519 서명 키

- 생성: `signing_key()` 최초 호출 시 OsRng로 생성(`service.rs:69`), 0600으로 저장(`store.rs:216`), 로드 시 공개키-개인키 일치 검증(`service.rs:51`).
- 소비: 그랜트 서명(`service.rs:913`) → 릴레이 검증(`src-tauri/src/remote/account_grants.rs:38`). 미핀 릴레이는 전부 403으로 닫힌다(`account_grants.rs:42`). 공개키 배포 라우트는 `/api/account/v1/public-key`(`service.rs:1114`)다.
- **회전(신규)**: 서명 입력에 `key_id`(예: `kid_2026_09`)를 추가하고, 릴레이는 `FERRYX_RELAY_ACCOUNT_PUBLIC_KEYS=kid1:pub1,kid2:pub2` 목록으로 핀한다(`bin/relay.rs:44`는 현재 단일 값). 회전 순서: (1) 새 키 생성·공개, (2) 릴레이에 신·구 동시 핀, (3) 계정 서비스 서명 전환, (4) `kid1` 제거. 동시 핀 유지 기간은 ≥ 그랜트 TTL 600초(`service.rs:30`) + 배포 지터.
- 알 수 없는 `key_id`는 403, `key_id` 누락은 신규 배포 이후 400으로 닫는다(다운그레이드 재생 방지).

### 2.2 결제 제공자 키·웹훅 시크릿

- `PAYMENT_SECRET_KEY`(API 호출)와 `PAYMENT_WEBHOOK_SECRET`(서명 검증)은 **유닛 파일/명령행에 절대 쓰지 않는다**. systemd drop-in `payment.conf`에서 `EnvironmentFile=/etc/ferryx/secrets/payment.env`(0600)로 주입한다.
- 웹훅 검증 시크릿은 계정 스토어에 저장하지 않는다(시크릿과 데이터 분리). 회전은 제공자 대시보드에서 재발급 → 파일 교체 → `systemctl daemon-reload && systemctl restart ferryx-relay` 2스텝이며, 구 시크릿은 제공자 측 유예 기간 동안만 유지한다.

### 2.3 메일 제공자 키

- `create_production_mailer`(`src-tauri/src/account/mailer.rs:247`)가 `RESEND_API_KEY`(`mailer.rs:248`) → `FERRYX_MAIL_WEBHOOK_URL/TOKEN` → `FileMailer` 순으로 선택한다. 프로덕션은 `mail.conf` drop-in의 `EnvironmentFile`로 `RESEND_API_KEY`·`FERRYX_MAIL_FROM`을 주입한다.
- 키가 없으면 매직링크가 파일 스풀(`FileMailer`)로 떨어져 로그인 자체가 무력화되므로, 기동 시 `MAIL_FAILED` 헬스 프로브(§3)로 조기 경보한다.

### 2.4 systemd drop-in 규약(호스트가 이미 쓰는 방식)

- 프로덕션 릴레이는 `/etc/systemd/system/ferryx-relay.service`(`ExecStart=/home/indo/bin/ferryx-relay --port 8787`, `Restart=always`)에 drop-in 디렉터리를 사용하며, 현재 `debug.conf`(RUST_LOG)와 `machine-tokens.conf`(`FERRYX_RELAY_MACHINE_TOKENS`)가 있다(운영 기록: `notes/facts/ferryx-relay-live-deploy-omarchy.md` §Live edge topology). 오리진 전환도 drop-in으로 수행했다(`notes/facts/ferryx-relay-single-binary-and-headless-login.md` §1).
- 규약(신규 템플릿: `packaging/relay/dropins/{common,machine-tokens,mail,payment,store}.conf.example`, `scripts/relay/install-dropins.sh`): 시크릿은 `EnvironmentFile`(0600)로만, 유닛 본문에는 비시크릿 기본값만, 적용은 `daemon-reload` + `restart`, 롤백은 drop-in 파일 제거 후 재시작.

## 3. 관측가능성

### 3.1 로그(무PII 원칙)

- 절대 로그 금지: 이메일 원문, 토큰·해시 원값, 페어링 토큰, 라이선스 키 평문, 웹훅 서명.
- 허용 식별자: `user_id`(`usr_*`, 무작위 `service.rs:507`), `machine_record_id`/`machine_id`, `grant_id`, `license_key_id`, `key_id`, `provider_event_id`.
- 이벤트 스키마: `event`, `decision`, `reason`(에러 코드), `user_ref`, `machine_ref`, `ts`. 예: `license.verify.fail reason=LICENSE_EXPIRED user_ref=usr_…`, `trial.started plan=trial_days=14`, `trial.converted`, `webhook.rejected reason=WEBHOOK_SIGNATURE_INVALID`.
- 이메일이 꼭 필요하면 `LOG_SALT` 기반 HMAC 앞 16hex만(`email_ref`) 남긴다.

### 3.2 메트릭(카운터 중심)

| 메트릭 | 타입/라벨 | 의미 |
| --- | --- | --- |
| `trial_started_total` | counter | 트라이얼 개시 |
| `trial_expired_total` | counter | 트라이얼 만료(전환 실패 포함) |
| `trial_converted_total` | counter | 유료 전환 |
| `license_verify_failures_total` | counter{reason} | 라이선스 검증 실패(만료/서명/기기 상한) |
| `license_machine_limit_hits_total` | counter | 기기 상한 초과 시도 |
| `grant_delivery_failures_total` | counter{stage} | 그랜트 전달 실패(`GRANT_DELIVERY_FAILED`, `service.rs:913`) |
| `webhook_rejected_total` | counter{reason} | 웹훅 위조/중복 거부 |
| `account_login_rate_limited_total` | counter | `LOGIN_RATE_LIMITED`(`service.rs:581`) |
| `account_store_writes_total` | counter{backend} | 전환기 json/sqlite 쓰기량 비교 |

### 3.3 방출 지점

- 계정 이벤트: `login_request`(`service.rs:581`), `device_approve_get`(`service.rs:507`), `enroll`(`service.rs:782`), `issue_grant`(`service.rs:913`)에서 구조화 로그/카운터를 직접 방출한다.
- 라이선스/빌링: 신규 `account/license.rs`·`account/payments.rs`(웹훅 핸들러 진입·거부 지점).
- 릴레이: 제어 소켓 인증 성공/실패(`relay_server.rs:2394`, `relay_server.rs:2348`), 그랜트 전달 타임아웃(`relay_server.rs:142`/`relay_server.rs:727`).
- 노출: `/metrics`는 루프백 전용(릴레이는 공개 포트 8787 하나이므로 별도 바인드 `127.0.0.1:8788`). 로그는 `tracing`(초기화 `bin/relay.rs:105`) → journald, 보존 30일.

## 4. 보안/어뷰징 — 위협 모델

| 위협 | 영향 | 완화 | 집행 지점 |
| --- | --- | --- | --- |
| 라이선스 키 공유(1키 N기기) | 매출 손실 | 서명 라이선스에 기기 상한·machine_id 바인딩, 활성화 시 상한 검사, 오프라인 유예(14일) 후 재검증 | (신규) `account/license.rs` + `license_keys.max_machines`; 발급은 `service.rs:913` 계열 관리 경로 |
| 트라이얼 파밍(다중 계정) | 매출 손실·비용 | 매직링크 이메일 실소유 검증 필수, 결제수단 지문 1회 트라이얼, machine_id 기반 trial claims UNIQUE | (신규) `trial_claims` 테이블 + `subscriptions` UNIQUE(user_id); 웹훅 경로 `account/payments.rs` |
| 시계 조작(로컬 시간 되돌리기) | 라이선스 무기한 사용 | 모든 만료는 서버 시각(`store.rs:16`)으로 발급, 클라이언트는 `last_seen_at`(machines, `store.rs:72`) 갱신 + 서버 시각 대비 skew 로그, 유예 초과 시 재검증 강제 | `store.rs:16`; (신규) 클라이언트 검증기 `license.verify` |
| 기기 스푸핑(machine_id 위조) | 그랜트·라이선스 탈취 | enroll 서명 검증(Ed25519, `service.rs:782`), machine_id 전역 유일·계정 클레임 충돌 시 409(`service.rs:782`), 재등록 시 epoch 증가 | `service.rs:782`; `machines_machine_id_unique` 인덱스 |
| 웹훅 위조 | 구독/결제 상태 위조 | 제공자 서명 검증 + 타임스탬프 창, payload는 sha256만 저장(PCI 회피), 중복 이벤트 멱등 | (신규) `account/payments.rs` + `payment_events_provider_event_unique` |
| 재전송(replay) | 그랜트/코드 재사용 | 도메인 분리 서명(`account_grants.rs:14`, `account_grants.rs:26`), 챌린지 nonce 1회 소비(`service.rs:181`), device 코드 1회 소비, 그랜트 TTL 600초(`service.rs:30`), 제어 챌린지 nonce+audience ±60초(`relay_server.rs:2394`) | `account_grants.rs:38`, `service.rs:181`, `relay_server.rs:2394` |
| 릴레이 제어 소켓 무단 접속/DoS | 터널 탈취·가용성 | 머신 토큰 미설정 시 전면 거부(`relay_server.rs:678`), Ed25519 바인딩 재사용 금지(`relay_server.rs:2348`), IP admission 429(`relay_server.rs:2348`), 만료 세션 10초 스윕(`relay_server.rs:69`) | `relay_server.rs:678`, `relay_server.rs:2348`, `relay_server.rs:2394` |
| 계정 API 브루트포스 | 계정 탈취 | 이메일당 시간당 5회(`store.rs:13`, `service.rs:149`), 본문 4096B(`store.rs:14`, `service.rs:1111`), 이메일 토큰 해시 필수(`service.rs:507`) | `service.rs:149`, `service.rs:507`, `service.rs:1111` |
| 오리진 위조/커트오버 사고 | 피싱·토큰 오발급 | https 강제(루프백 예외), `ferryx.dev`는 명시적 옵트인 전까지 거부(`origin.rs:59`, `origin.rs:77`) | `origin.rs:59`~`origin.rs:77` |

추가 규율: 라이선스 검증 실패는 **fail-closed**(유예 창 이후 실행 거부)지만, 그랜트 전달 실패는 발급 롤백(`service.rs:913`)이라 유저 상태가 "무효 토큰 보유"로 남지 않는다.

## 5. 셀프호스팅 운영

- **설치(로컬 전용 정책 준수)**: 릴리스 자동화는 호스팅 CI에서 금지된다(`AGENTS.md:103`~`AGENTS.md:106`, `scripts/release-workflow-policy.mjs:18`, `:63`, `:202`). 릴레이는 서명이 필요 없는 서버 바이너리이므로 `cargo build --release --bin ferryx-relay`로 소스에서 직접 빌드하거나 유지관리자의 로컬 릴리스 산출물을 받아 `/home/<user>/bin/ferryx-relay`에 복사한다(데스크톱 앱 배포 토폴로지는 `docs/releases/LOCAL_RELEASE_RUNBOOK.md:3`~`:5`, `:13`~`:17`).
- **단위 파일/기동**: systemd 유닛 + drop-in(§2.4), 기본 포트 8787(`bin/relay.rs:30`), 데이터 디렉터리 `~/.ferryx/account-data`(`bin/relay.rs:92`) 또는 `FERRYX_ACCOUNT_DATA_DIR`(`bin/relay.rs:133`).
- **계정 서비스 없이 실행**: 오늘은 릴레이가 항상 계정 라우터를 병합하고 데이터 디렉터리가 없으면 종료한다(`bin/relay.rs:153`, `relay_server.rs:2233`). (신규) `FERRYX_DEPLOYMENT_MODE=selfhost` 또는 `FERRYX_DEPLOYMENT_MODE=selfhost`을 추가해 `relay_router_with_account(state, key, None)` 경로로 기동하고, 이때 계정 API 라우트는 404로 남긴다. 이 모드에서 라이선스 검증은 수행하지 않는다(라이선스 서버 없는 셀프호스트는 기존과 동일하게 전 기능 사용).
- **텔레마트리**: 기본 **없음**(결정 D2). 로그는 journald에 로컬 보존(`bin/relay.rs:105`), `/metrics`는 루프백 전용, 아웃바운드 전화(home) 없음. 텔레마트리를 켜는 배포는 운영자 옵트인이어야 한다.
- **라이선스 서버 없는 업그레이드**: 라이선스는 계정 공개키로 오프라인 검증되는 서명 블롭이므로, 릴레이/데몬 업그레이드에 라이선스 서버 접촉이 필요 없다. 데스크톱 데몬은 UDS 롤링 핸드오버를 사용한다: 승격 판정 `src-tauri/src/daemon/handover.rs:57`, 상태 기계 `src-tauri/src/daemon/handover_transaction.rs:58`, FD 전달 프레임 `src-tauri/src/daemon/handover_socket.rs:16`~`handover_socket.rs:20`, 소켓 경로 `handover_socket.rs:76`, 0700 소켓 `handover_socket.rs:86`. 릴레이는 핸드오버 없이 바이너리 교체 + 유닛 재시작이며, 재시작 동안 데몬 제어 채널은 자동 재연결된다(운영 기록: `notes/ferryx-account-grants-audit-and-regressions-2026-09-25.md` §11).

## 6. 결정

### D1. SQLite 이전 시점

- 옵션 A: 즉시 이전(사용자 6명 규모) — 이득 없음, 마이그레이션/백업 리스크만 선지출.
- 옵션 B(권장): **T1 시점(첫 결제 경로 착지 릴리스)에 이전**, T2~T4는 조기 경보 트리거.
- 옵션 C: 규모 임계(T4) 도달 시 이전 — 그 시점엔 결제 멱등성·UNIQUE 요구가 이미 프로덕션 사고로 나타난 뒤다.
- 권장 근거: §1.1의 한계는 스케일이 아니라 correctness다. `payment_events` 멱등성은 문서 규율이 아니라 DB 제약이어야 하고, 그 요구가 생기는 순간이 첫 유료 고객이다. 반대로 그 전에는 flock 한 줄(`store.rs:195`)이 6 users에서 정확하다.

### D2. 텔레마트리 정책

- 옵션 A: 기본 수집(opt-out) — 전환율 분석은 좋지만, "릴레이는 평문을 보지 않는다"는 아키텍처 약속과 셀프호스팅 신뢰를 훼손한다.
- 옵션 B(권장): **기본 수집 없음, 명시적 opt-in만**. 계정 API를 운영하는 공식 호스팅에서만 서버측 이벤트(§3)를 수집하고, 셀프호스트/데스크톱은 로컬 로그만 남긴다.
- 권장 근거: 상용 지표는 공식 호스팅의 서버측 데이터(가입·전환·검증 실패)로 충분히 얻어진다. 클라이언트 텔레마트리를 켜지 않아도 §3 메트릭은 전부 확보된다.
- 옵션 C: 익명 집계만 기본 수집 — 여전히 동의 없는 아웃바운드이며, 엔터프라이즈 셀프호스트 도입 심사를 불필요하게 막는다.

## 7. 단계 계획 (경로/함수 + 인수 테스트)

- **P1 스토어**: `src-tauri/src/account/store_sqlite.rs`(신규: `SqliteStore`, `open_or_import`, `export_json_snapshot`) + `store.rs`에 백엔드 선택 추가 + `src-tauri/src/bin/account.rs`에 `Migrate { dry_run }`. 락 규율은 `lock_account_dir`(`store.rs:195`) 유지.
- **P2 키 회전**: `account_grants.rs`의 `GrantSubmission`에 `key_id` 추가(현재 `account_grants.rs:38`은 단일 핀), `relay_server.rs:2167` 핀 목록화, `service.rs:69` 키 세대 관리.
- **P3 커머스**: `account/payments.rs`(`handle_webhook`), `account/license.rs`(`issue_license`/`verify_license`), `service.rs:1111` 라우터에 `/api/account/v1/billing/webhook`·`/licenses` 추가, `subscriptions`/`payment_events`/`license_keys` 테이블.
- **P4 관측**: `account/metrics.rs` + 루프백 `/metrics`, §3 이벤트명 고정.
- **P5 셀프호스팅**: `bin/relay.rs`에 `FERRYX_DEPLOYMENT_MODE=selfhost`, `packaging/relay/` 템플릿, `scripts/relay/{install-dropins.sh,restore-drill.sh}`.

인수 테스트(리터럴 명령 + 기대 관측):

1. 마이그레이션 단위: `cargo test --manifest-path src-tauri/Cargo.toml --lib account::store_sqlite` → `json_store_migrates_without_losing_rows`, `migration_is_idempotent`, `unique_machine_id_is_enforced` 포함 `test result: ok. N passed; 0 failed`, EXIT=0.
2. 드라이런: `FERRYX_ACCOUNT_DATA_DIR=/tmp/ferryx-mig-fixture cargo run --manifest-path src-tauri/Cargo.toml --bin ferryx-account -- migrate --dry-run` → `MIGRATION_DRY_RUN users=6 machines=3` 출력, `account-store.json` mtime 불변. 이어 `--apply` 실행 시 `MIGRATION_APPLIED users=6 machines=3 version=1`.
3. 복구 리허설: `scripts/relay/restore-drill.sh --backup /var/lib/ferryx/backups/account-latest.db` → `RESTORE_DRILL_PASS integrity=ok users=6 machines=3`, EXIT=0.
4. 어뷰징 회귀: `cargo test --manifest-path src-tauri/Cargo.toml --lib account::license` → `license_beyond_machine_limit_is_refused`, `expired_license_fails_closed`; `cargo test --manifest-path src-tauri/Cargo.toml --lib account::payments` → `forged_webhook_signature_is_rejected`, `replayed_provider_event_is_applied_once`.
5. 키 회전 회귀: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::account_grants` → `unknown_key_id_is_rejected`, `previous_key_id_is_accepted_during_rotation` (기존 `account_grants.rs:89` 게이트 테스트와 함께 green).
6. 무PII 로그 게이트: 마이그레이션+로그인 리허설 후 `! grep -q '@' /tmp/ferryx-relay.log` (이메일 원문 미출력), `grep -c 'MIGRATION_APPLIED' /tmp/ferryx-relay.log` == 1.

</details>

### 3.13 보안 / 남용 방지
- 위협 모델(키 공유·체험 파밍·시계 조작·기기 스푸핑·웹훅 위조·리플레이) → 상세: `06-ops-data-security.md`

### 3.14 단계별 실행 계획 + 수용 테스트
- P0(소유 관계) → P1(라이선스 코어) → P2(강제 지점) → P3(결제) → P4(SQLite 승격) → P5(관측/남용) → P6(문서/셀프호스트 배포)
- 각 단계는 파일 경로·함수·수용 테스트를 명시한다(레인 상세 병합).

## 4. 결정 로그 (Decision Log)

> 형식: 각 결정은 **옵션 2개 이상 + 권고 + 근거**를 갖는다. 레인 상세 원문은 §3.x 파일 참조.

### D1. LS/라이선스 검증 방식 — 오프라인 서명 검증 (권고)
- 옵션 A: 매 기동마다 서버 질의(온라인 전용). 옵션 B(권고): **Ed25519 서명 라이선스 + 주기적 갱신**.
- 근거: 오프라인·폐쇄망에서 데스크톱이 동작해야 하고, 기존 grant 검증 프리미티브(`account_grants.rs:26,38`)를 그대로 재사용할 수 있다. 서버 질의는 relay 가용성에 로컬 기능을 묶는다(설계 불변식 1 위반).

### D2. 강제 위치 — relay 전용 (권고), 데몬 이중 강제 금지
- 옵션 A: relay + daemon 이중 강제. 옵션 B(권고): **relay 경계에서만**.
- 근거: 데몬은 계정/라이선스 의존이 0건이며(레인 L2 실측: `license` 식별자 0), 소유권 진실 원천은 account store다. 이중 강제는 셀프호스트 경로를 깨고 검증 표면을 2배로 만든다. 데몬에 정책이 필요해지면 기존 봉인 배달 경로(`deliver_grant_offer`)에 서명된 claim을 실어 보낸다(이번 단계 미구현).

### D3. 라이선스 없이 데몬 기동 — 항상 허용 (권고)
- 옵션 A: 데몬이 라이선스 없으면 기동 거부. 옵션 B(권고): **항상 기동, 강제는 relay 경계에서만**.
- 근거: 셀프호스팅 영구무료 약속의 핵심. 라이선스 부재는 정상 상태다.

### D4. 만료 시 차단 범위 — 신규 원격 연결만 (권고)
- 옵션 A: 기존 세션까지 즉시 종료. 옵션 B(권고): **신규 attach/pair만 차단**, 진행 중 세션은 유지.
- 근거: 실패는 열화로(설계 불변식 4). 진행 중 터미널을 끊으면 사용자 데이터·작업이 손실된다.

### D5. 클라이언트 캐시/오프라인 — 서명 스냅샷 + TTL (권고)
- 옵션 A: 캐시 없음(매번 온라인). 옵션 B(권고): **서명된 라이선스 스냅샷 로컬 보관**, TTL 24h, grace 14일, degrade-only.
- 근거: 네트워크 단절 시 로컬 기능이 죽으면 안 된다. 서명이 있으므로 로컬 변조는 검증에서 걸린다.

### D6. MoR(결제 대행) 선택 — Paddle 권고, Lemon Squeezy 차선
- 옵션: Paddle(권고) / Lemon Squeezy / Stripe(직접 세금 처리).
- 근거: 글로벌 VAT·세금 대행(MoR)이 필요하고 한국 개인/법인 취급이 단순하다. Stripe는 세금·사업자 이슈를 직접 부담한다. **세부 비교·확정은 §3.7(03-payment-mor.md) 결과로 대체**(레인 L3).

### D7. 데이터 스토어 전환 시점 — 결제 이벤트 도입 시 SQLite (권고)
- 옵션 A: JSON 유지 + 락 강화. 옵션 B(권고): **결제/구독 레코드 도입과 동시에 SQLite 승격**.
- 근거: 돈이 걸린 이벤트는 부분 손상 허용 불가. 전환 전까지는 현행 JSON+락(`store.rs:132,150,161,195`)으로 충분(users=6).

### D8. 셀프호스트 정의 — "자체 relay + 로컬 전부 무료·영구" (권고)
- 옵션 A: 셀프호스트도 계정/라이선스 요구. 옵션 B(권고): **`FERRYX_DEPLOYMENT_MODE=selfhost`를 명시한 배포에서만** 무계정 경로 허용(설정 부재로 자동 강등 금지 — sol-6 B1 수용).
- 근거: 제품 약속(영구 무료)과 현행 구조(계정 라우터가 relay에만 마운트, `bin/relay.rs:149`)를 동시에 만족. 회귀 테스트로 고정한다(§6 T-SELFHOST).

### D9. 남용 대응 — 다층 방어 (권고)
- 옵션 A: 단일 지표 기반 차단. 옵션 B(권고): **기기 지문 + 결제수단 중복 + 이메일 평판 + 서버측 체험 시각**의 조합.
- 근거: 어느 한 지표도 단독으로는 오탐/우회가 쉽다. 세부 위협모델은 §3.13(06-ops-data-security.md).

## 5. 단계별 실행 계획 (Phases)

| 단계 | 목표 | 주요 파일/함수 | 수용 테스트 |
|---|---|---|---|
| **P0. 소유 관계** | 기기↔계정 바인딩(게이트의 전제) | `src-tauri/src/account/store.rs`(machines에 `user_id`), `account/service.rs`(enroll/목록), `remote/relay_server.rs`(소유권 판정) | T-OWN |
| **P1. 라이선스 코어** | 서명 라이선스 발급/검증(3모드) | `src-tauri/src/account/license.rs`(신규), `account/service.rs`(라우트: `/license/issue`, `/license/verify`, `/public-key`), `remote/account_grants.rs`(검증 재사용) | T-LICENSE |
| **P2. 서버 강제** | 등록·attach·프록시에 entitlement/소유 검사 | `remote/relay_server.rs`(등록 경로, `/api/v1/attach/session`, `/host/{machine}` 프록시, tunnel/control), `account/service.rs`(`ErrorBody` 확장) | T-ENFORCE |
| **P3. 30일 체험** | 체험 시작/만료/유예 서버 판정 + 알림 | `account/store.rs`(trial 레코드), `account/service.rs`(체험 상태), `account/mailer.rs`(템플릿 확장) | T-TRIAL |
| **P4. 결제/MoR** | 구독·웹훅·상태머신 | `src-tauri/src/account/billing.rs`(신규), `account/service.rs`(`/billing/checkout`, `/billing/webhook`) | T-BILLING |
| **P5. 스토어 승격** | JSON→SQLite + 백업/복원 | `account/store.rs`(스키마 버전, SQLite 백엔드), 마이그레이션 스크립트 | T-MIGRATE |
| **P6. 관측/남용/문서** | 메트릭·로그·셀프호스트 배포 문서 | `relay_server.rs`(로그), `docs/`(운영 문서), `scripts/`(셀프호스트 안내) | T-OBS |

## 6. 수용 테스트 (Acceptance)

| ID | 시나리오 | 리터럴 명령 | 기대 관측 |
|---|---|---|---|
| **T-OWN** | 계정 A가 등록한 기기를 계정 B가 attach 시도 | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::attach_rejects_non_owner` | 403 `OWNERSHIP_MISMATCH` |
| **T-LICENSE** | 서명 라이선스 검증(정상/위조/만료) | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::license::tests` | 정상=Ok, 위조·만료=Err |
| **T-TRIAL** | 체험 30일 + **3일 read-only grace** 경과 후 신규 attach | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::trial_grace_then_block_new_attach` | D+30~D+33: 신규 attach **허용(읽기 전용 표시)**; D+33 이후 **403** `TRIAL_EXPIRED` (402 미사용) |
| **T-SELFHOST** | **`FERRYX_DEPLOYMENT_MODE=selfhost`로 기동한 무계정 relay**에서 원격 attach | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::selfhost_without_account_service_allows_attach` | 성공(라이선스 검사 미수행) — **회귀 방지 핵심** |
| **T-ENFORCE** | 기기 수 한도 초과 등록 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::enroll_beyond_seats_reports_entitlement_exceeded` | 409 `ENTITLEMENT_EXCEEDED` + `details:{seats,used}` |
| **T-BILLING** | MoR 웹훅 서명 검증 + 멱등 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::webhook_signature_and_idempotency` | 위조 서명=401, 중복 이벤트=1회 반영 |
| **T-MIGRATE** | JSON→SQLite 마이그레이션 **+ 실제 롤백 + 손실 0 검증** | 단일 리터럴: `rm -rf /tmp/mig-test && cp -r ~/.ferryx/account-data /tmp/mig-test && scripts/relay/migrate-json-to-sqlite.sh --data-dir /tmp/mig-test --apply --verify --rollback --verify-rollback` | 스크립트가 순서대로 ① dry-run(변경 0, `rows=users:N,machines:M`) ② apply(SQLite 생성, JSON 원본 보존) ③ verify(행 수·SHA-256 일치, exit 0) ④ **rollback 실행**(SQLite→JSON 역추출 후 원본과 병합, 전환 이후 쓰기 0 확인) ⑤ verify-rollback(병합 결과 == 원본 + 신규 쓰기, 손실 0). 각 단계 실패 시 exit ≠ 0 이고 원본 JSON은 불변 |
| **T-UI** | 라이선스 상태 9종 렌더 | `bun run --cwd ui test src/state/licenseStore.test.ts` | 전 상태 렌더, 로컬 차단 0 |
| **T-LOCAL** | 계정/라이선스 없이 로컬 사용 | `bun run --cwd ui test src/components/settings/PlanSection.test.tsx` | 계정 프롬프트 0회, 로컬 기능 정상 |
| **T-DEPLOY-MODE** | commercial 모드에서 계정 키 부재로 기동 | 단일 명령: `FERRYX_DEPLOYMENT_MODE=commercial FERRYX_RELAY_ACCOUNT_PUBLIC_KEY= timeout 20 cargo run --manifest-path src-tauri/Cargo.toml --bin ferryx-relay & sleep 3; curl -s -o /tmp/r.json -w '%{http_code}' -X POST -H 'content-type: application/json' -d '{"machineId":"m1"}' http://127.0.0.1:8787/api/v1/attach/session; cat /tmp/r.json; kill %1` | stdout **503** + body `{"code":"DEPLOYMENT_MISCONFIGURED"}`, 기존 세션 유지 |
| **T-SELFHOST-ENROLL** | `selfhost` 모드 무계정 relay에서 등록→페어링→attach→터널 4단계 | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::selfhost_enrollment_through_tunnel` | 4단계 모두 성공(계정 라우터 미마운트 상태) |
| **T-TUNNEL-NEG** | capability 없이 **4개 터널 경로 전부** 직접 호출 / 비소유자 프록시 | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::tunnel_requires_capability_for_all_routes` | `/tunnel/control`, `/tunnel/data/{s}`, `/tunnel/client/{s}`, `/tunnel/opaque/{s}` 각각 capability 없음=**401**; 비소유자 `/host/{machine}`=**404**(의도적 차이: 인증 실패 401 vs 존재 비노출 404) |
| **T-LICENSE-BIND** | 라이선스를 타 계정·타 deployment로 재사용 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::license::tests::license_is_bound_to_account_and_deployment` | 위조/재사용 모두 Err |
| **T-ROTATE** | 키 회전 overlap/retirement | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::license::tests::key_rotation_overlap_then_retire` | overlap 중 구·신 key_id 모두 통과, retire 후 구 키 실패 |
| **T-TRIAL-START** | 가입만 한 계정은 체험 미시작 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::trial_starts_on_first_enroll_not_signup` | `trial_started_at` null → 등록 후 세팅 |
| **T-TRIAL-RESET** | 동일 지문 + 신규 계정 재설치 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::trial_reset_denied_for_seen_fingerprint` | **403** `TRIAL_ALREADY_USED` |
| **T-BILLING-MAP** | 웹훅 이벤트 → entitlement 매핑 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::event_to_entitlement_mapping` | 표와 일치(refunded/chargeback → 즉시 다운그레이드) |
| **T-BILLING-DUNNING** | D7 경과 시 차단 범위 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::billing::tests::dunning_day7_blocks_new_attach_only` | 신규 attach만 **403**, 기존 세션 유지 |
| **T-MIGRATE-CRASH** | 임포트 중/플립 전/후 강제 종료 후 **롤백까지** | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::store_sqlite::tests::crash_at_each_cutover_boundary` | 재기동 후 행 수·해시 일치, 손실 0 |
| **T-LEGACY-CLAIM** | 레거시 기기 자기 claim 시도 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::service::tests::legacy_machine_cannot_self_claim` | **403** `LEGACY_CLAIM_FORBIDDEN` |
| **T-QUOTA** | 한도·동시 분·자정·재시작·하위경로 (개별 케이스) | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::` (아래 5개) | 각 케이스 기대값 명시 |
| ↳ device_limit | 디바이스 한도 초과 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::device_limit_rejects_extra_device` | **409** `ENTITLEMENT_EXCEEDED` + `details:{limit,used}` |
| ↳ feature_denial | 미포함 feature 요청 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::feature_not_in_plan_is_denied` | **403** `FEATURE_NOT_ENTITLED` |
| ↳ concurrent_minutes | 동시 세션 분 합산 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::concurrent_sessions_consume_minutes_aggregated` | 2세션 60초 → 카운터 **+2분**(합산), 초과 시 신규 세션만 거부 |
| ↳ midnight_rollover | UTC 자정 리셋 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::counter_resets_at_utc_midnight` | 23:59 소비 후 00:00 카운터 **0** |
| ↳ relay_restart | 재기동 후 카운터 보존 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::counter_survives_relay_restart` | 재기동 후 소비량 동일 |
| ↳ tunnel_bypass | 하위 터널 직접 접근 | `cargo test --manifest-path src-tauri/Cargo.toml --lib account::quota::tests::direct_tunnel_access_does_not_bypass_quota` | capability 없음 **401**, 쿼터 미소비 |
| ↳ capability_binding | capability의 계정·기기·세션 바인딩 | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::capability_is_bound_to_account_machine_session` | 타 계정·타 기기·타 세션으로 재사용 시 **401** |
| ↳ capability_expiry | capability 만료(≤60초) | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::capability_expires_after_ttl` | TTL 경과 후 **401** |
| ↳ capability_single_use | capability 이식·재사용 거부 | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::capability_is_not_transferable` | 동일 capability 2회 사용 시 두 번째 **401** |
| ↳ capability_no_quota_bypass | **유효 capability라도 쿼터/entitlement를 우회 못 함** | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::valid_capability_cannot_bypass_quota_or_entitlement` | 쿼터 소진·feature 미포함 상태에서 유효 capability로 터널 개설 시도 → **403** ENTITLEMENT_EXCEEDED / FEATURE_NOT_ENTITLED (capability는 인증 수단일 뿐 권한 부여가 아님) |

## 7. 마이그레이션·호환 (기존 사용자·기존 배포 보존)

| 대상 | 현재 상태(실측) | 마이그레이션 계약 |
|---|---|---|
| 운영 계정 스토어 | JSON 1파일(`account-store.json`, users=6 / machines=3) | 스키마 버전 필드 추가 → 리더가 구버전 허용 → SQLite 전환 시 1회 복사 + 원본 보존(되돌림 가능) |
| 기존 기기(machines 3대) | 소유자(user_id) 없음 | **선착순 귀속 금지(sol-6 B8 수용)**: 운영자가 서명한 바인딩(`machine→user`) 또는 운영자 승인 절차로만 귀속한다. 미귀속 기기는 `legacy: true`로 남고 원격 attach는 운영자 승인 전까지 차단 |
| 기존 relay 운영자(셀프호스팅) | 계정 서비스 미설정으로 동작 | **영구 무료 경로 보존**하되 **명시적 모드가 필요**하다(sol-6 신규B 수용): 업그레이드 절차 = ① 서비스 drop-in에 `FERRYX_DEPLOYMENT_MODE=selfhost` 추가 → ② `systemctl daemon-reload && restart` → ③ 기동 로그에서 `deployment_mode=selfhost` 확인. 이 절차 없이 새 바이너리만 올리면 기동이 **거부**된다(무단 fail-open 방지) |
| 기존 데스크톱 설치본 | 계정 없이 로컬 동작 | 로컬 기능 무변경. 원격 기능만 상태에 따라 배지/차단 |
| CLI/설치 스크립트 | `install.sh`/`install.ps1`이 relay에서 서빙 | 라이선스 도입 후에도 설치 경로 무변경(라이선스는 런타임 개념) |
| 릴리스 정책 | 로컬 전용(`AGENTS.md:104`) | 유지 — 결제/라이선스도 릴리스 자동화를 요구하지 않는다 |

**호환 불변식**: (1) **명시적 `selfhost` 모드를 설정한** relay는 오늘과 동일하게 무계정으로 동작한다(설정 절차는 위 표). (2) 로컬 전용 사용자는 어떤 프롬프트도 보지 않는다. (3) 기존 기기는 데이터 손실 없이 귀속된다.

## 8. 리스크·미해결 (초안)

| # | 리스크 | 영향 | 완화 |
|---|---|---|---|
| R1 | 셀프호스팅 경로에 라이선스 검사가 새어 들어감 | 자체호스팅 신뢰 붕괴(제품 약속 위반) | 회귀 테스트: `FERRYX_DEPLOYMENT_MODE=selfhost`로 기동한 무계정 relay에서 원격 attach가 그대로 통과해야 한다(수용 테스트 필수). 모드 미설정 기동은 거부되므로 "설정 누락"이 이 경로를 열지 않는다. |
| R2 | 오프라인 사용자의 체험 판정 왜곡(시계 조작) | 매출 누수 | 서버 발급 `not_before` + 마지막 확인 시각 저장 + 유예 후 재확인 |
| R3 | MoR 심사 지연(사업자·약관) | 출시 지연 | 결제 전 단계(P0~P2 게이트)를 먼저 출시하고 결제는 후속 |
| R4 | JSON 스토어가 결제 이벤트로 손상 | 정산 불일치 | 결제 이벤트는 append-only + 웹훅 idempotency + SQLite 전환 |
| R5 | 체험 악용(다계정) | 매출 누수·비용 | 기기 지문·결제수단 중복 탐지·이메일 도메인 평판 |
| R6 | 기존 사용자 이탈(갑작스런 게이트) | 신뢰 하락 | 유예 기간 공지 + 로컬 기능 무제한 보장 명시 |

**사용자 확정 대기(옵션·권고·근거는 아래 Decision 블록 — sol-6 B11 수용)**

### D10. 가격·무료 한도 수치 (사용자 확정 대기)
- 옵션 A: 개인 1티어 — **월 $12**(MoR 수수료 5%+$0.50 하한 때문에 $8은 실수령이 과소), 기기 3 / 디바이스 5 / 릴레이 120분·일. 옵션 B: 2티어 — 개인 $12(기기 3·디바이스 5), 팀 $29(기기 10·디바이스 20·공유 grant).
- **권고: A로 시작(출시가 월 $12 — §3.7의 MoR 수수료 하한과 단일 값으로 일치)**(측정 전 과금 구조 단순화, 업그레이드는 데이터 확보 후). 근거: 현재 계정 6·기기 3 규모에서 2티어는 관리 비용만 늘고, MoR 심사·세금 처리도 단순해야 한다. 수치는 첫 30일 실측(체험→전환율) 후 재조정.

### D11. MoR 최종 확정 (사용자 확정 대기)
- 옵션: Paddle(권고) / Lemon Squeezy(폴백) / Stripe(직접 세금).
- **권고: Paddle.** 근거: 한국 사업자 이슈 없이 VAT 대행, 5%+$0.50, 구독·체험·다우닝 지원. 확정 전 필요한 실무: 계정 개설·약관·환불정책 페이지(사이트 배포).

### D12. 셀프호스트 추가 제한 여부 (사용자 확정 대기)
- 옵션 A(권고): **제한 없음** — 자체 relay + 로컬 전부 영구 무료, 공식 호스팅 relay만 유료. 옵션 B: 자체 relay는 무료이나 공식 릴레이 네트워크 페더레이션은 유료.
- **권고: A.** 근거: 제품 약속을 단순·신뢰 가능하게 유지(셀프호스트 사용자를 적대하지 않음). B는 페더레이션 구현이 선행돼야 한다.

## 8.1 리뷰 준비 상태

- 산출물: `docs/plans/RELAY_LICENSING_TRIAL_SELFHOST_PLAN_2026-09-25.md` (동일 내용, 커밋 경로)
- 검증 완료: C1 커버리지(전 주제 헤딩 + 1,821줄), C2 인용 12건 실측, C3 결정 40건·경로 76개, C5 마크다운 전용
- 미해결(사용자 결정): MoR 확정, 무료/유료 한도 수치, 셀프호스트 추가 제한 여부
- PARKED(별건): Linux arm64 CLI 빌드 재개 조건



## 11. 라우트별 권한 매트릭스 (sol-6 B3 수용)

대상: `relay_server.rs:2182-2224` 전 경로. 표기: **A**=인증 요구, **O**=소유권 검사, **E**=entitlement/라이선스, **Q**=쿼터, **S**=기존 세션 보존 정책.

| 라우트 | A | O | E | Q | S | 미인증/비소유 시 응답 |
|---|---|---|---|---|---|---|
| `POST /api/v1/pair/exchange` | ✔ | ✔ | ✔ | – | – | 401 / 403 `OWNERSHIP_MISMATCH` |
| `POST /api/v1/attach/session` | ✔ | ✔ | ✔ | ✔ | 기존 세션 유지 | 401 / 403 `TRIAL_EXPIRED`·`ENTITLEMENT_EXCEEDED` |
| `/api/v1/...` (기기·권한 계열) | ✔ | ✔ | ✔ | – | – | 401 / 404 |
| `/host/{machine_id}/api/v1/{*path}` | ✔ | **✔(신규)** | ✔ | ✔ | – | 401 / **404**(존재 비노출) |
| `GET /tunnel/control` | ✔(capability) | ✔ | ✔ | – | – | **401** |
| `GET /tunnel/data/{session_id}` | ✔(capability) | ✔ | ✔ | ✔ | 진행 중 세션 허용 | **401** |
| `GET /tunnel/client/{session_id}` | ✔(capability) | ✔ | ✔ | ✔ | 진행 중 세션 허용 | **401** |
| `GET /tunnel/opaque/{session_id}` | ✔(capability) | ✔ | ✔ | ✔ | 진행 중 세션 허용 | **401** |
| `GET /install.sh`, `/install.ps1`, `/download/*` | – | – | – | – | – | 공개(설치 경로는 게이트하지 않음) |

**선행 예외(우선순위 명시 — sol-6 R6 수용)**: `FERRYX_DEPLOYMENT_MODE=commercial`에서 기동 검증(계정 공개키·서명 키·데이터스토어 연결)이 실패한 경우, 그 판정은 **인증·소유권·entitlement 검사보다 먼저** 평가되어 **신규 enroll·attach에 503 `DEPLOYMENT_MISCONFIGURED`** 를 반환한다. 즉 아래 매트릭스의 401/403/404는 **배포 검증이 통과한 뒤의** 규칙이며, 진행 중 세션은 이 예외에서도 유지된다.

**capability 규칙(신규)**: `/tunnel/*` 4경로는 **인가된 attach 이후에만 발급되는 단명(≤60초)·대상 한정·기기 한정 capability**를 요구한다. capability는 계정·기기·세션에 바인딩되며 재사용·이식이 불가하다. 라우트를 직접 호출하면 401이고, 그 호출은 쿼터를 소비하지 않는다.

**응답 코드 차이의 의도**: 인증 실패는 **401**, 자원 존재를 숨겨야 하는 비소유 접근은 **404**(T-OWN의 attach는 대상이 이미 공개된 흐름이므로 **403**). 이 차이는 의도된 설계이며 권한 매트릭스가 유일한 기준이다.

## 10. 외부 리뷰 대응 (gpt-5.6-sol 라운드 1: BLOCK, 블로커 11건 처분)

> 리뷰 원문: `.omo/evidence/relay-licensing-plan/review-plan-gpt-5.6-sol.md` (VERDICT: BLOCK).
> 아래는 각 블로커를 **수용(설계 반영)** 또는 **반박(근거 제시)**으로 처분한 기록이다.

### 리뷰 라운드 요약 (gpt-5.6-sol, 총 7라운드)

| 라운드 | 판정 | 지적 | 처분 |
|---|---|---|---|
| R1 | BLOCK | 블로커 11건 | 전부 수용·수정(B1~B11) |
| R2 | BLOCK | 부분해결 5·미해결 3·신규 2 | §11 라우트 매트릭스 신설, 수용 테스트 리터럴화, D10~D12 승격 |
| R3 | BLOCK | 섹션 간 모순 5 + 신규 2 | 체험 grace 3일·가격 $12·selfhost 모드 단일 기준 통일, capability 테스트 3종 |
| R4 | BLOCK | 잔여 5건 | §3.3 모드조건, §2 표 회귀, 환불/차지백 전이표(B6b), `...::` 제거, 우회 방지 테스트 |
| R5 | BLOCK | 잔여 3건 | R1 문구, T-MIGRATE 단일 리터럴+실제 롤백, 지문 운영계약(B9b) |
| R6 | BLOCK | 잔여 2건 | 지문 삭제↔재체험 모순(tombstone 확정), 503 선행 예외 |
| **R7** | **APPROVE-WITH-NOTES** | **블로커 none** | 노트 3건 반영(참조 정정, PII 표현 완화, 만료 기준 명시) |

원문 증적: `.omo/evidence/relay-licensing-plan/review-plan{,-round2..7}-gpt-5.6-sol.md`

### B1. 상용 강제가 "설정 부재"로 fail-open — **수용**
- 조치: §2 불변식 4 신설 + D8 개정. `FERRYX_DEPLOYMENT_MODE=commercial|selfhost` **필수, 기본값 없음**. `commercial`은 기동 시 계정 공개키·서명 키·데이터스토어 연결을 검증하고 실패 시 **신규 기기 등록·신규 원격 attach에 fail-closed**. `selfhost`만 무계정 허용.
- 수용 테스트 추가: **T-DEPLOY-MODE** — `commercial` 모드에서 계정 키를 제거하고 기동 → 신규 enroll/attach가 503 `DEPLOYMENT_MISCONFIGURED`로 거부되고 **기존 세션은 유지**되어야 한다.

### B2. 정적 토큰 폐기 시 self-host 등록 경로 소실 — **수용**
- 조치: §3.5에 **등록 프로토콜 2종 분리** 명시. commercial = 계정 세션 + 기기 소유 증명 + 소유 바인딩 + entitlement, selfhost = **운영자 발급 등록 시크릿(현행 `FERRYX_RELAY_MACHINE_TOKENS`)을 selfhost 전용으로 존속**.
- 수용 테스트 추가: **T-SELFHOST-ENROLL** — 무계정 relay에서 `enroll → pair/exchange → attach → tunnel` 4단계가 순서대로 성공한다(현행 T-SELFHOST는 attach만 검증 → 확장).

### B3. 터널 계열 경로 우회 — **수용**
- 조치: **라우트별 권한 매트릭스**를 §3.5에 추가(relay_server.rs:2182-2224 전 경로 × {인증, 소유권, entitlement, 쿼터, 기존 세션}).
- 핵심 신규 규칙: `/tunnel/control|data|client|opaque`는 **인가된 attach 이후에만 발급되는 단명(≤60초)·대상 한정·기기 한정 capability**를 요구한다. capability 없이 호출하면 401, 비소유자는 404(존재 비노출).
- 수용 테스트 추가: **T-TUNNEL-NEG** — capability 없이 `/tunnel/data/{session}` 호출 → 401; 타 계정 소유 기기 id로 `/host/{machine}` 호출 → 404.

### B4. 라이선스 공유·리플레이·폐기·키 회전 미정의 — **수용**
- 조치(§3.1 개정): 라이선스에 `subject_account_id`, `deployment_id`, `machine_binding`, `nonce`, `key_id`를 **필수**로 넣고, 서명 입력에 **도메인 분리 프리픽스**를 둔다(기존 grant 서명과 교차 사용 불가). 신뢰 키는 `key_id`로 인덱싱한 **키링**으로 보관하며, 회전은 **중복(overlap) 기간**과 **긴급 폐기 절차**를 갖는다.
- 폐기 지연 상한을 명시: **온라인 ≤15분**(짧은 리스 + 폐기 epoch), **오프라인 ≤24시간**(TTL). 환불·차지백·키 유출은 **epoch 증가**로 즉시 반영.
- 만약 라이선스가 relay 내부 구현 세부라면 **클라이언트 강제 문구를 제거**하고 "relay가 모든 라이선스를 인증된 계정에 독립 바인딩한다"로 정리한다(문서 일관성).
- 수용 테스트 추가: **T-LICENSE-BIND** — A계정 라이선스를 B계정/다른 deployment로 재사용 시 거부; **T-ROTATE** — 키 회전 중 구·신 key_id 라이선스가 overlap 기간에 모두 통과하고, retirement 후 구 키는 실패.

### B5. 체험 시작 정의 불일치 — **수용**
- 조치: §3.2에 **단일 정의** 확정(첫 성공 `/machines/enroll`), §0의 "가입 후 30일" 문구를 "**첫 기기 등록 후 30일**"로 정정.
- 수용 테스트 추가: **T-TRIAL-START** — 가입만 하고 등록하지 않으면 `trial_started_at`이 비어 있고, 등록 시점에 시작된다; **T-TRIAL-RESET** — 동일 지문으로 재설치+신규 계정 → 체험 거부(180일 창).

### B9b. 지문 이력 운영·프라이버시 계약 (sol-6 R5 신규 수용)

- **보존·삭제(단일 계약으로 확정 — 모순 제거)**: 원본 지문 이력 `trial_fingerprints`는 `first_seen_at + 180일`에 자동 삭제된다. **삭제 요청 시에도 재체험 방지는 리셋되지 않는다**: 삭제 시 해당 항목을 지우는 대신 **억제 tombstone**(keyed-HMAC 앞 16바이트만, 목적=사기 방지, **보존=최초 관측일(first_seen_at) + 180일** — 삭제 요청일부터 새 180일이 시작되지 않는다, 법적 근거=정당한 이익·부정 사용 방지)으로 **치환**한다. tombstone은 **원문 지문을 포함하지 않는 최소 가명처리 레코드**이며(HMAC 앞 16바이트), 180일 만료 시 완전 삭제된다. 접근·보존·삭제 제한은 아래 항목을 따른다.
- **삭제 요청 처리(확정)**: 요청 즉시 `trial_fingerprints` 원본은 삭제하고 tombstone으로 치환한다(활성 체험/구독 중이어도 동일 — 보류 규칙 없음). 감사 로그에 "삭제 요청 → tombstone 치환"을 기록한다.
- **일관 규칙**: 재체험 방지 창(180일)은 **tombstone이 유지하는 값**이며, 원본 삭제 여부와 무관하게 동일하다. "삭제하면 신규로 취급된다"는 서술은 이 계약으로 대체된다(문서 내 해당 표현 제거).
- **서버 키 회전**: `trial_fingerprints`의 HMAC 키는 **연 단위 회전**하며, 회전 시 기존 항목은 구 키로 검증 가능한 상태로 유지(키 ID 태깅)한 뒤 새 지문부터 신 키를 쓴다.
- **접근 권한·감사**: 원문 지문은 저장하지 않고 HMAC 앞 16바이트만 보관하며, 이 테이블 접근은 **운영자 역할**로 제한한다. 조회·삭제·override는 모두 감사 로그에 기록한다.
- **오탐 이의제기·운영자 override**: 오탐(공용 장비·VM 재사용 등)으로 체험이 거부된 사용자는 이의제기 경로를 가지며, 운영자는 근거를 남기고 **1회성 체험 승인**을 발급할 수 있다(감사 로그 필수).

### B6b. 환불·차지백 전이표 (sol-6 R4-6 수용 — 상태 머신에 실제로 존재)

| 웹훅 이벤트 | SubscriptionStatus 전이 | entitlement 효과 | 기존 세션 | revocation epoch |
|---|---|---|---|---|
| subscription.created | (none) → trialing | 체험 entitlement | 유지 | – |
| subscription.activated | trialing/past_due → active | 유료 entitlement 부여 | 유지 | – |
| transaction.payment_failed | active → past_due | 7일 grace 동안 유지 | 유지 | – |
| subscription.canceled | active → canceled | 기간 종료까지 유지 | 유지 | – |
| transaction.refunded | * → refunded | **즉시 제거** | **유지** | **+1** |
| transaction.chargeback | * → chargeback | **즉시 제거** | **유지** | **+1** |

- SubscriptionStatus enum에 refunded, chargeback을 **명시적으로 추가**한다(문서가 주장만 하고 모델에 없던 결함 해소).
- 단일 규칙: 환불·차지백은 **entitlement 즉시 제거 + epoch 증가**, 진행 중 세션은 데이터 보호를 위해 유지하되 신규 연결은 차단된다.

### B6. 결제가 entitlement를 구동하기에 불충분 — **수용**
- 조치(§3.7 개정): **웹훅 이벤트 → entitlement 매핑 표**를 추가하고, **일할 계산(proration)**, **다우닝 스케줄(D0/D3/D7, D7 종료 시 유예 종료)**, **환불/차지백 → 즉시 다운그레이드 + epoch 증가**를 명시. 상태 머신에 `refunded`, `chargeback`을 추가한다.
- 수용 테스트 추가: **T-BILLING-MAP** — 각 이벤트(activated/past_due/canceled/refunded) 후 entitlement 결과가 표와 일치; **T-BILLING-DUNNING** — D7 경과 시 신규 attach만 차단, 기존 세션 유지.

### B7. SQLite 전환 순서가 안전 결정과 모순 — **수정**
- 조치(§3.6 개정): 순서를 **정지(writes quiesce, flock) → 스냅샷 → 임포트 → 검증 → 플립 → 이후 쓰기는 SQLite**로 고정하고, **롤백은 전환 이후 쓰기를 버리지 않는다**(SQLite → JSON 역추출 후 원본 JSON과 병합). 각 경계에서 **크래시 주입 테스트**를 필수로 한다(행 수/해시 비교만으로는 불충분).
- 수용 테스트 추가: **T-MIGRATE-CRASH** — 임포트 중·플립 직전·플립 직후 각 지점에서 강제 종료 후 재기동 시 데이터 손실 0.

### B8. 레거시 기기 귀속의 계정 탈취 위험 — **수용**
- 조치: §7 개정(선착순 금지). 운영자 서명 바인딩 또는 운영자 승인으로만 귀속, 미귀속 기기는 원격 attach 차단.
- 수용 테스트 추가: **T-LEGACY-CLAIM** — 임의 계정이 레거시 기기를 스스로 claim 시도 → 403.

### B9. entitlement가 이름만 있고 사양·테스트 부족 — **수용**
- 조치(§3.4 개정): 각 entitlement에 **단위, 리셋 타임존(UTC 자정), 카운터 소유자(relay 데이터스토어), 원자적 예약/회계, 동시 세션 동작, 실패 복구, 초과(overage) 정책**을 정의.
- 수용 테스트 추가: **T-QUOTA** — 디바이스 한도, 기능 거부, **동시 분 소비**, **자정 롤오버**, **relay 재시작 후 카운터 보존**, 하위 터널 직접 접근 차단.

### B10. 수용 테스트가 리터럴이 아니거나 불완전 — **수정**
- 조치: §6의 모든 항목에 **리터럴 명령 + 기대 관측(코드/바디/로그)**을 명시하고, 위에서 추가한 테스트를 표에 병합한다. "402/403"처럼 모호한 기대는 **단일 상태코드**로 확정한다.

### B11. 선언된 미해결 결정이 문서 자체 기준 미달 — **수정**
- 조치: §8의 "미해결(사용자 결정 필요)" 3건을 **각각 옵션·권고·근거**를 갖춘 Decision 블록으로 승격한다(가격·한도 수치, MoR 최종 확정, 셀프호스트 추가 제한). 미결정 상태는 "**사용자 확정 대기**"로 라벨링하되 옵션·권고는 명시한다.

### 리뷰 노트(비블로커) 처분
- 세션 최대 수명 + 자격증명 유출 시 **긴급 폐기 규칙**을 D4에 부기(수용).
- 상태코드 단일화(수용, B10에서 반영).
- 기기 지문·이메일 평판 통제에 **개인정보 고지·보존기간·삭제·오탐 이의제기** 절차 추가(수용, §3.13).
- 운영 수치(users=6/machines=3)에 **측정 시각·환경 라벨** 부기, 용량 근거로 사용 금지(수용, §1에 이미 라벨링 + 명시).
- 결제·서명키 릴리스에 **2인 체크리스트·아티팩트 출처·롤백 절차** 요구(수용, §3.12).

## 9. 인용 검증 기록 (C2)
아래 file:line 인용을 이 세션에서 직접 실행해 확인했다(증적: `.omo/evidence/relay-licensing-plan/c2-citations.txt`).

| # | 인용 | 확인 방법 | 기대 내용 |
|---|---|---|---|
| 1 | `src-tauri/src/bin/relay.rs:149` | sed -n | `AccountState::new(&data_dir, &origin, mailer)` |
| 2 | `src-tauri/src/account/store.rs:132` | sed -n | `pub struct AccountStore` |
| 3 | `src-tauri/src/account/store.rs:195` | sed -n | `pub fn lock_account_dir` |
| 4 | `src-tauri/src/account/store.rs:10-14` | sed -n | TTL·한도 상수 5개 |
| 5 | `src-tauri/src/remote/account_grants.rs:38` | sed -n | `pub fn verify_grant_signature` |
| 6 | `src-tauri/src/remote/relay_server.rs:2182` | sed -n | `.route("/api/v1/pair/exchange"…)` |
| 7 | `src-tauri/src/remote/relay_server.rs:286` | sed -n | `machine_tokens: Vec<String>` |
| 8 | `src-tauri/src/account/mailer.rs:183` | sed -n | `.post("https://api.resend.com/emails")` |
| 9 | `src-tauri/src/account/service.rs:1114-1132` | sed -n | 계정 라우트 14종 |
| 10 | `ui/src/remote/accountSession.ts:197` | sed -n | `export async function resolveAccountOrigin` |
| 11 | `src-tauri/src/daemon/protocol.rs:1592` | sed -n | `{"type":"upgradeBinary"}` |
| 12 | `AGENTS.md:104` | sed -n | 로컬 전용 릴리스 정책 |
