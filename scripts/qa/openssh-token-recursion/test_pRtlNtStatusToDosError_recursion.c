#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifndef STATUS_ASSERTION_FAILURE
#define STATUS_ASSERTION_FAILURE ((NTSTATUS)0xC0000420L)
#endif

#ifndef STATUS_NO_SUCH_USER
#define STATUS_NO_SUCH_USER ((NTSTATUS)0xC0000064L)
#endif

#ifndef ERROR_NO_SUCH_USER
#define ERROR_NO_SUCH_USER 1317L
#endif

#ifndef STATUS_ACCESS_DENIED
#define STATUS_ACCESS_DENIED ((NTSTATUS)0xC0000022L)
#endif

#ifndef ERROR_ACCESS_DENIED
#define ERROR_ACCESS_DENIED 5L
#endif

#ifndef STATUS_INVALID_HANDLE
#define STATUS_INVALID_HANDLE ((NTSTATUS)0xC0000008L)
#endif

#ifndef ERROR_INVALID_HANDLE
#define ERROR_INVALID_HANDLE 6L
#endif

typedef ULONG(NTAPI *RtlNtStatusToDosErrorType)(NTSTATUS);

static int g_load_ntdll_calls = 0;
static int g_get_proc_address_calls = 0;

static HMODULE load_ntdll(void)
{
    g_load_ntdll_calls++;
    return GetModuleHandleA("ntdll.dll");
}

static RtlNtStatusToDosErrorType get_proc_address(HMODULE hm, const char* name)
{
    g_get_proc_address_calls++;
    FARPROC raw = GetProcAddress(hm, name);
    RtlNtStatusToDosErrorType typed_fn = NULL;
    _Static_assert(sizeof(raw) == sizeof(typed_fn), "Pointer size mismatch between FARPROC and RtlNtStatusToDosErrorType");
    if (raw != NULL) {
        memcpy(&typed_fn, &raw, sizeof(typed_fn));
    }
    return typed_fn;
}

ULONG pRtlNtStatusToDosError(NTSTATUS status)
{	
	HMODULE hm = NULL;
	typedef ULONG(NTAPI *RtlNtStatusToDosErrorType)(NTSTATUS);
	static RtlNtStatusToDosErrorType s_pRtlNtStatusToDosError = NULL;

	if (!s_pRtlNtStatusToDosError) {
		if ((hm = load_ntdll()) == NULL)
			return STATUS_ASSERTION_FAILURE;

		if ((s_pRtlNtStatusToDosError = (RtlNtStatusToDosErrorType)get_proc_address(hm, "RtlNtStatusToDosError")) == NULL)
			return STATUS_ASSERTION_FAILURE;
	}	
#ifdef TEST_FIXED_VERSION
	return s_pRtlNtStatusToDosError(status);
#else
	return pRtlNtStatusToDosError(status);
#endif
}

static int RunChildWorker(void)
{
    ULONG err_user = pRtlNtStatusToDosError(STATUS_NO_SUCH_USER);
    if (err_user != ERROR_NO_SUCH_USER) {
        fprintf(stderr, "FAIL: expected %lu got %lu\n",
                (unsigned long)ERROR_NO_SUCH_USER, (unsigned long)err_user);
        return 10;
    }

    ULONG err_access = pRtlNtStatusToDosError(STATUS_ACCESS_DENIED);
    if (err_access != ERROR_ACCESS_DENIED) {
        fprintf(stderr, "FAIL: expected %lu got %lu\n",
                (unsigned long)ERROR_ACCESS_DENIED, (unsigned long)err_access);
        return 11;
    }

    ULONG err_handle = pRtlNtStatusToDosError(STATUS_INVALID_HANDLE);
    if (err_handle != ERROR_INVALID_HANDLE) {
        fprintf(stderr, "FAIL: expected %lu got %lu\n",
                (unsigned long)ERROR_INVALID_HANDLE, (unsigned long)err_handle);
        return 12;
    }

    if (g_load_ntdll_calls != 1 || g_get_proc_address_calls != 1) {
        fprintf(stderr, "FAIL: cache miss load_calls=%d proc_calls=%d\n",
                g_load_ntdll_calls, g_get_proc_address_calls);
        return 13;
    }

    printf("CHILD_SUCCESS: status_no_such_user=%lu status_access_denied=%lu status_invalid_handle=%lu\n",
           (unsigned long)err_user, (unsigned long)err_access, (unsigned long)err_handle);
    return 0;
}

int main(int argc, char* argv[])
{
    if (argc > 1 && strcmp(argv[1], "--child") == 0) {
        return RunChildWorker();
    }

    char exePath[MAX_PATH];
    if (!GetModuleFileNameA(NULL, exePath, MAX_PATH)) {
        fprintf(stderr, "GetModuleFileName failed: %lu\n", (unsigned long)GetLastError());
        return 1;
    }

    char cmdLine[MAX_PATH + 32];
    snprintf(cmdLine, sizeof(cmdLine), "\"%s\" --child", exePath);

    STARTUPINFOA si;
    PROCESS_INFORMATION pi;
    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    ZeroMemory(&pi, sizeof(pi));

    if (!CreateProcessA(NULL, cmdLine, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
        fprintf(stderr, "CreateProcess failed: %lu\n", (unsigned long)GetLastError());
        return 1;
    }

    DWORD waitResult = WaitForSingleObject(pi.hProcess, 1500);
    if (waitResult == WAIT_FAILED) {
        fprintf(stderr, "WaitForSingleObject failed: %lu\n", (unsigned long)GetLastError());
        TerminateProcess(pi.hProcess, 1);
        WaitForSingleObject(pi.hProcess, 1000);
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        return 1;
    }

#ifdef TEST_FIXED_VERSION
    if (waitResult == WAIT_TIMEOUT) {
        fprintf(stderr, "FAIL: post-fix child timed out unexpectedly\n");
        TerminateProcess(pi.hProcess, 1);
        WaitForSingleObject(pi.hProcess, 1000);
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        return 2;
    }

    DWORD exitCode = 1;
    GetExitCodeProcess(pi.hProcess, &exitCode);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);

    if (exitCode != 0) {
        fprintf(stderr, "FAIL: post-fix child exited with code %lu\n", (unsigned long)exitCode);
        return 3;
    }

    printf("[PASS] Post-fix child succeeded with exit code 0. Error mapping and loader caching verified.\n");
    return 0;
#else
    if (waitResult == WAIT_TIMEOUT) {
        printf("[CONFIRMED DEFECT] Pre-fix child timed out after 1500ms as expected due to infinite recursion.\n");
        TerminateProcess(pi.hProcess, 99);
        WaitForSingleObject(pi.hProcess, 1000);
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        return 0;
    }

    DWORD exitCode = 0;
    GetExitCodeProcess(pi.hProcess, &exitCode);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);

    fprintf(stderr, "FAIL: pre-fix child finished prematurely without timeout (exitCode=%lu)\n", (unsigned long)exitCode);
    return 4;
#endif
}
