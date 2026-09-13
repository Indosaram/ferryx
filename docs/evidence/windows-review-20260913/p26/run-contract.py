#!/usr/bin/env python3
"""Execute production permission policy without Cargo or desktop operations.

Select Windows/Linux cfg branches on Darwin; remove serialization derives only.
This does not qualify native ABI, serialization or the Tauri command registry.
"""
import hashlib
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[4]
SOURCE = ROOT / 'src-tauri/src/permissions/mod.rs'
TEST = ROOT / 'src-tauri/tests/permissions_contract.rs'
MODEL = ROOT / 'src-tauri/src/notification/model.rs'


def prepare(text):
    text = text.replace('use serde::{Deserialize, Serialize};\n', '')
    text = re.sub(r'^#\[serde\([^\n]*\)\]\n', '', text, flags=re.M)
    text = text.replace(', Serialize, Deserialize', '')
    return text.replace('target_os = ', 'p26_os = ')


source = SOURCE.read_text()
tests = TEST.read_text()
model = MODEL.read_text()
# The real model enum/DTO implementations, no substitute permission mapping.
model = model[model.rfind('#[derive', 0, model.index('pub enum NotificationPlatform')):model.index('/// Result of an explicit permission request.')]
print('source_sha256=' + hashlib.sha256(source.encode()).hexdigest(), flush=True)
print('test_sha256=' + hashlib.sha256(tests.encode()).hexdigest(), flush=True)
failed = False
with tempfile.TemporaryDirectory(prefix='ferryx-p26-') as directory:
    root = Path(directory)
    code = '''extern crate self as ferryx_lib;
mod util {
    pub fn no_window_command(_: &str) -> std::process::Command {
        panic!("P26 default desktop launcher must never execute")
    }
}
pub mod notification {
''' + prepare(model) + '''
    pub fn invalidate_permission_cache() {}
    pub struct Provider;
    pub fn platform_permission_provider() -> Provider { Provider }
    impl Provider {
        pub fn status(&self) -> NotificationPermissionStatusDto {
            NotificationPermissionStatusDto::non_authoritative(
                NotificationPlatform::current(), cfg!(p26_os = "windows"))
        }
    }
}
pub mod permissions {
''' + prepare(source) + '\n}\n' + prepare(tests)
    fixture = root / 'contract.rs'
    fixture.write_text(code)
    for platform in ['windows', 'linux']:
        binary = root / platform
        command = ['rustc', '--edition=2021', '--test', str(fixture), '--cfg', f'p26_os="{platform}"', '-o', str(binary)]
        print('COMMAND ' + repr(command), flush=True)
        built = subprocess.run(command, timeout=60, check=False)
        if built.returncode:
            raise RuntimeError('Compilation prerequisite failed; not behavioral RED')
        result = subprocess.run([str(binary), '--nocapture'], timeout=30, check=False)
        print(f'{platform} test_exit={result.returncode}', flush=True)
        failed |= result.returncode != 0
print(f'cleanup_owned_root_absent={not root.exists()}', flush=True)
raise SystemExit(1 if failed else 0)
