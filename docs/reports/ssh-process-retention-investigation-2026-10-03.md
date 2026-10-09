# SSH 잔여 프로세스 조사 종료 기록

작성일: 2026-10-03

상태: 사용자 요청으로 조사 중단. 문서화 완료이며 장애 해결 완료가 아니다.

## 결론과 한계

Windows maho-win에서 확인한 고CPU `sshd.exe` 3개는 OpenSSH의 인증 준비 단계에서 오류 변환 함수의 자기재귀에 빠져 있었다. 이 프로세스들의 무한 반복은 실제 스택과 코드로 확인했다.

그러나 **Ferryx 데몬이 종료해야 할 SSH 연결을 계속 보유하는 별도 문제가 없다고 입증하지 못했다.** 오류 발생을 Ferryx의 연결·취소·재시도 패턴이 유발하거나 증폭하는지도 미확인이다. "우리가 연결을 종료하지 않는다는 증거를 찾지 못했다"와 "데몬이 연결을 보유할 가능성이 없다"는 다른 주장이다. 후자는 이 조사로 뒷받침되지 않는다.

## 확인한 기존 장애

- 대상: Windows OpenSSH `C:\Windows\System32\OpenSSH\sshd.exe`, FileVersion `9.5.6.2`, ProductVersion `OpenSSH_9.5p2`.
- 최초 관찰 PID: `8760`, `15092`. 이후 실험 시작 전부터 존재하던 `21056`에서도 동일 스택을 확인했다. 이 PID들은 당시 기록이며 현재 정리 대상으로 재사용하면 안 된다.
- 최초 두 프로세스에는 TCP 소켓과 자식 프로세스가 없었고, CPU 시간이 계속 증가했다. Unix의 종료된 Z 프로세스가 아니라 살아 있는 CPU 루프였다.
- 비침습 CDB 스택: `privsep_preauth -> _posix_spawn_asuser -> get_user_token -> generate_sshd_virtual_token -> pRtlNtStatusToDosError -> GetProcAddress`.
- 오류 코드: `0xc0000064`, `STATUS_NO_SUCH_USER`.
- 오류 변환 함수는 얻어 둔 Windows API 함수 포인터 대신 자기 자신을 호출한다. 디스어셈블리에서도 반복 분기를 확인했다.

```c
// 관찰한 upstream 결함
return pRtlNtStatusToDosError(status);
// 함수 포인터를 호출하는 수정 형태
return s_pRtlNtStatusToDosError(status);
```

소스 근거: [PowerShell/openssh-portable, 고정 리비전의 w32api_proxies.c](https://github.com/PowerShell/openssh-portable/blob/b8c08ef9da9450a94a9c5ef717d96a7bd83f3332/contrib/win32/win32compat/w32api_proxies.c#L246-L259).

함수 수준 Windows 회귀 실험에서는 수정 전 호출이 1,500ms 제한을 넘겼고, 수정 후에는 오류 값 `1317`, `5`, `6`과 로더 캐싱을 확인했다. 컴파일과 회귀 실행은 exit 0이었다. 이는 함수 결함의 증거이지, 수정된 전체 sshd의 운영 검증은 아니다. 전체 수정 서버 빌드·배포는 완료하지 않았다.

## 실제 수행한 연결 비교

운영 서비스와 분리한 동일 inbox sshd를 SYSTEM 예약 작업으로 실행했다. 리스너는 `127.0.0.1:40222`, 인증은 전용 테스트 키와 기존 `sook` 계정을 사용했다. 운영 포트 22의 설정과 사용자 SSH 프로필은 변경하지 않았다.

| 실험 | 결과 | 증거/제한 |
| --- | --- | --- |
| 직접 Windows SSH 순차 연결·종료 20회 | 20회 성공 | `DIRECT_SEQUENTIAL_20_PASS`, exit 0, 세션 기록 `bash_63` |
| 직접 Windows SSH 4개 동시 연결, 5묶음 | 20회 성공 | `CONCURRENT_20_PASS`, exit 0, `bash_64` |
| 서버 KEXINIT 패킷 수신 후 소켓 취소, 일반 SSH로 재연결 | 20쌍 성공 | `WIRE_KEX_CANCEL_RETRY_20_PASS`, exit 0, `bash_67`; 원시 소켓 취소이며 Ferryx 취소 경로는 아님 |
| 위 비교 후 서버 자식 잔류 | sshd 자식 0개 | `REMAINING_SSHD_CHILDREN=0`, `bash_68`; 이어 작업 제거 및 포트 리스너 0 확인 |
| 실제 BridgeConnection + SSH 실행 | 프레임 읽기 타임아웃 | 서버 로그에서 공개키 인증 및 원격 명령 시작 확인. 실패 후 sshd 자식 0개 |
| 테스트 SSH 래퍼 제거 후 BridgeConnection::from_child | 같은 타임아웃 | `SSH_DIRECT_EXIT=1`; 래퍼만으로 실패를 설명할 수 없음 |
| production BridgeConnection::spawn 대조 | 같은 타임아웃 | `PRODUCTION_SPAWN_EXIT=1`, `bash_86`; 전체 앱 수명주기 시험은 아님 |
| 로컬 헬퍼 직접 실행 | 핸드셰이크 성공 | 원격 워커의 `BRIDGE_OK` 및 exit 0 보고 |
| 로컬 PowerShell 래핑 헬퍼 실행 | 핸드셰이크 성공 | 원격 워커의 `BRIDGE_OK` 및 exit 0 보고; PowerShell 단독 실패 가설은 지지하지 않음 |

실제 브리지 실행기는 Windows의 기존 `C:\fx-ptyfix-gate\target\debug\deps\libferryx_lib.rlib`에 연결했다. 현재 설치 앱 전체와 동일한 산출물임을 증명한 시험은 아니다. 전체 앱의 터미널 닫기, 재연결, 핸드오버를 동일 조건으로 반복하는 비교는 끝내지 못했다.

원래 관찰한 장애는 **인증 전 무한 반복**이다. 브리지 실험에서 만난 것은 **인증 후 프레임 응답 타임아웃**이다. 둘을 같은 원인으로 취급하거나 후자를 원래 장애의 재현이라고 보고해서는 안 된다.

## 실험 실패와 진행상 문제

- 초기 하네스의 인증 설정, 호스트 키 ACL, 예약 작업 명령, 환경변수와 헬퍼 수명 관리가 잘못되어 준비 단계가 길어졌다.
- Python 및 stderr 리다이렉션을 사용하는 시험은 타임아웃이 났고, 직접 실행은 통과했다. 차이의 원인은 확정하지 못했다.
- 도중 Windows 재부팅이 확인됐다. 확인된 부팅 시각은 `2026-10-03T01:33:16.500Z`이며, 이 조사에서 재부팅 명령은 실행하지 않았다. 재부팅 원인은 조사하지 않았다.
- 컴파일되지 않거나 실행되지 않은 하네스를 워커가 반복 반환했다. 실행 증거가 없는 준비 작업을 진척으로 전달한 것이 지연을 키웠다.
- 원래 장애의 발생 조건을 추적하기보다 하네스 타임아웃 디버깅으로 범위가 이동했다. 원격 PowerShell을 우회하는 마지막 SSH 대조는 사용자 중지 요청으로 완료하지 않았다.

## 데몬 연결 보유 가능성이 남는 이유

관찰 당시 Ferryx의 연결 개수는 활성 원격 터미널당 제어·출력용 두 연결과 맞았다. 이는 한 시점의 수량 일치일 뿐이다. 다음은 검증하지 못했다.

- 실제 앱에서 터미널을 닫았을 때 해당 SSH PID가 종료되는지.
- 연결 취소·실패·재연결 후 이전 세대의 SSH PID가 남지 않는지.
- 데몬 핸드오버 이후 연결 소유권과 종료 책임이 정확히 이전되는지.
- 특정 세션에 연결되지 않은 SSH 프로세스를 데몬이 계속 참조하는지.

따라서 현재 결론은 **확인한 고CPU 서버 프로세스의 직접 원인은 OpenSSH 결함이고, 별도의 Ferryx 데몬 연결 보유 문제는 미확인**이다.

## 중단 및 정리 상태

사용자의 "작업 다 중지 해" 요청으로 워커를 취소했다. 최종 종료 명령에서 전용 예약 작업 `FerryxSshCausal-58ae8e76d39e4921ada1f5cb490c57a1`을 중지·제거했고 `OWN_TEST_TASK_STOPPED`, exit 0을 확인했다(`bash_88`). 최종 취소 이후 모든 원격 프로세스를 다시 전수 조사하지는 않았다. 앞선 완료된 시험에서는 잔여 sshd 자식 0과 소유 헬퍼 정리를 확인했다.

운영 SSH 교체, 운영 데몬 강제 종료, GUI 조작, 운영 수정 배포는 수행하지 않았다. 이 조사에서 프로덕션 수정 커밋을 만들지 않았다. 공유 작업 트리의 다른 세션 변경은 그대로 보존했다.

다음은 미커밋 조사 산출물이며, 재검토 없이 검증된 자동화로 취급하면 안 된다.

- `scripts/qa/openssh-token-recursion/`: upstream 한 줄 패치, 함수 회귀 실험 및 미완료 전체 서버 빌드 도구.
- `scripts/qa/ssh-causal-comparison/`: 초기 Windows 비교 하네스. 실제 통과한 비교는 수정된 별도 PowerShell 명령으로 실행했다.
- `scripts/qa/ssh-ferryx-comparison/`: 캐시 라이브러리 기반 진단 실행기와 SSH 래퍼. 성공한 전체 SSH 핸드셰이크 비교는 없음.
- 로컬 임시 증거: `/tmp/sshd-bridge.log`, `/tmp/sshd-ready.log`, `/tmp/sequential-01-4480.stderr.log`, `/tmp/file-output-control.log`. 임시 경로이므로 영구 보관을 보장하지 않는다.
- 원격 전용 증거/산출물 디렉터리는 삭제 완료를 주장하지 않는다. `C:\ProgramData\Ferryx\ssh-causal-evidence\run-20261003-062907-31128632` 및 `C:\Users\sook\AppData\Local\Temp\ferryx-bridge-driver-20261003`에는 테스트 키와 진단 산출물이 남을 수 있다. 키 내용은 보고서에 포함하지 않는다.

## 재개한다면

재개는 새 지시에 따른다. 우선 실제 사용 경로에서 세션 ID, 연결 시도 ID, 로컬 SSH PID, 생성·취소·재시도·종료 시각을 서버 접속 및 sshd PID와 대응시켜야 한다. 새 잔여 프로세스가 생길 때 스택과 연결 상태를 확보해 어느 소유권 경계에서 종료가 빠졌는지 확인하는 편이 현재 하네스를 계속 확장하는 것보다 직접적이다.

발생하지 않은 관찰 기간만으로 누수가 없다고 결론 내리지 않는다. 이전 PID를 재사용하거나 모든 sshd를 일괄 종료하지 않는다. 관찰용 변경, 배포 또는 정리 작업은 이 문서화 요청에 포함하지 않았다.
