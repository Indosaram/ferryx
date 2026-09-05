# Ferryx 및 파트너 센터 앱 스토어 제출용 필수 식별자 명세 (Store Submission Identifiers Specification)

**문서 갱신일:** 2026-09-05  
**대상 계정:** Microsoft Partner Center (`Project Maho`)  
**공통 Publisher:** `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`  
**공통 PublisherDisplayName:** `Project Maho`  

---

## 1. Microsoft Partner Center 등록 완료 앱 식별자 목록

| 앱 이름 | Package/Identity/Name | Package Family Name (PFN) | Publisher ID | PublisherDisplayName |
|---|---|---|---|---|
| **Ferryx** | `ProjectMaho.Ferryx` | `ProjectMaho.Ferryx_s4dtschhe0d3e` | `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36` | `Project Maho` |
| **Noveling** | `ProjectMaho.Noveling` | `ProjectMaho.Noveling_s4dtschhe0d3e` | `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36` | `Project Maho` |
| **Maho Browser** | `ProjectMaho.MahoBrowser` | `ProjectMaho.MahoBrowser_s4dtschhe0d3e` | `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36` | `Project Maho` |

---

## 2. Ferryx 프로젝트 적용 상태 (`orca-lite`)

### 2.1 패키지 매니페스트 (`src-tauri/windows/msix/AppxManifest.xml`)
```xml
<Identity
  Name="ProjectMaho.Ferryx"
  Publisher="CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36"
  Version="0.1.0.0"
  ProcessorArchitecture="x64" />

<Properties>
  <DisplayName>Ferryx</DisplayName>
  <PublisherDisplayName>Project Maho</PublisherDisplayName>
  <Logo>Assets\StoreLogo.png</Logo>
  <Description>Ultra-lightweight workspace and AI agent launcher powered by Tauri v2 and Rust</Description>
</Properties>
```

### 2.2 빌드 스크립트 기본 파라미터 (`scripts/build-msix.ps1`)
- `$Publisher = "CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36"`
- `$PackageName = "ProjectMaho.Ferryx"`

### 2.3 Windows Store 필수 이미지 에셋 (`src-tauri/icons/`)
- `StoreLogo.png` (50x50): 스토어 목록 및 제품 상세 표시용 (준비 완료)
- `Square44x44Logo.png`: 시작 메뉴 앱 목록 및 작업 표시줄 (준비 완료)
- `Square71x71Logo.png`: 소형 타일 (준비 완료)
- `Square150x150Logo.png`: 중형 타일 (준비 완료)
- `Square310x310Logo.png`: 대형 타일 및 스플래시 스크린 (준비 완료)
- 추가 지원 에셋: `Square30x30Logo.png`, `Square89x89Logo.png`, `Square107x107Logo.png`, `Square142x142Logo.png`, `Square284x284Logo.png` 완비

---

## 3. Maho Browser 프로젝트 적용 위치 (`maho-workspace`)

- **Manifest Template**: `maho-chromium/build/windows_msix/AppxManifest.xml.in`
  - `<PublisherDisplayName>Project Maho</PublisherDisplayName>`
- **Packaging Script**: `maho-chromium/build/scripts/package_windows_msix.py`
  - `_DEFAULT_PUBLISHER = "CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36"`
  - `_DEFAULT_PACKAGE_NAME = "ProjectMaho.MahoBrowser"`

---

## 4. Noveling 프로젝트 적용 위치 (`noveling`)

- **Package Name**: `ProjectMaho.Noveling`
- **Publisher**: `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`
- **Publisher Display Name**: `Project Maho`

---

## 5. 법적 및 정책 필수 URL (스토어 등록정보 기재 항목)

- **공식 웹사이트 (Website URL)**: `https://ferryx.app`
- **개인정보처리방침 (Privacy Policy URL)**: `https://ferryx.app/privacy`
- **지원/이슈 트래커 (Support URL)**: `https://github.com/Indosaram/ferryx/issues`
- **저작권 문구 (Copyright)**: `Copyright © 2026 Ferryx`
- **게시자 표시 (Publisher)**: `Project Maho`
