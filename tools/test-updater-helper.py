"""After cargo build --release, test the helper with disposable windowless fixtures.

Run from the repository root: python tools/test-updater-helper.py
Only generated fixture processes/files and an isolated APPDATA directory are used.
Artifacts are retained under .codex-tmp for inspection; no recursive cleanup.
"""
from pathlib import Path
import hashlib, json, os, shutil, subprocess, sys, time

workspace = Path(__file__).resolve().parents[1]
root = workspace / '.codex-tmp' / ('update-helper-smoke-' + str(time.time_ns()))
root.mkdir()
assert root.resolve().is_relative_to((workspace / '.codex-tmp').resolve())
helper = workspace / 'target/release/ShiTu.exe'
source = root / 'fixture.rs'
source.write_text(r'''
#![windows_subsystem = "windows"]
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "--update-started" {
        let dir = std::path::PathBuf::from(&args[2]);
        if dir.join("crash").exists() { std::process::exit(7); }
        std::fs::write(dir.join("started"), b"ready").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
    } else {
        std::fs::write(std::env::current_exe().unwrap().with_extension("restarted"), b"old restarted").unwrap();
    }
}
''', encoding='utf-8')
fixture = root / 'fixture.exe'
subprocess.run(['rustc', str(source), '-o', str(fixture)], check=True, creationflags=subprocess.CREATE_NO_WINDOW)

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def wait_for(predicate, timeout=20):
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        if predicate(): return
        time.sleep(.05)
    raise AssertionError('Timed out')

for scenario in ['success', 'startup_failure', 'no_commit', 'bad_hash']:
    case = root / scenario
    case.mkdir()
    stage = case / '.shitu-update-fixture'
    stage.mkdir()
    target = case / 'ShiTu.exe'
    shutil.copy2(fixture, target)
    # A harmless overlay makes the new executable byte-distinct.
    candidate = stage / 'new.exe'
    candidate.write_bytes(fixture.read_bytes() + b'new version test overlay')
    shutil.copy2(helper, stage / 'updater.exe')
    old_hash, new_hash = sha(target), sha(candidate)
    if scenario == 'startup_failure': (stage / 'crash').write_bytes(b'1')
    parent = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(90)'], creationflags=subprocess.CREATE_NO_WINDOW)
    plan = dict(target=str(target.resolve()), parent_pid=parent.pid, old_hash=old_hash,
                new_hash=('0'*64 if scenario == 'bad_hash' else new_hash), version='99.0.0')
    (stage / 'plan.json').write_text(json.dumps(plan), encoding='utf-8')
    env = dict(os.environ, APPDATA=str(case / 'profile'))
    worker = subprocess.Popen([str(stage / 'updater.exe'), '--apply-update', str(stage / 'plan.json')],
                              env=env, creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        if scenario == 'bad_hash':
            assert worker.wait(timeout=20) == 1
            assert not (stage / 'ready').exists() and sha(target) == old_hash
        else:
            wait_for(lambda: (stage / 'ready').exists() or worker.poll() is not None)
            assert (stage / 'ready').exists(), f'helper startup failed in {scenario}'
            assert sha(target) == old_hash, 'target touched before commit/parent exit'
            if scenario != 'no_commit': (stage / 'commit').write_bytes(b'update')
            parent.terminate(); parent.wait(timeout=10)
            result = worker.wait(timeout=40)
            if scenario == 'success':
                assert result == 0 and sha(target) == new_hash and sha(stage / 'old.exe') == old_hash
                assert (stage / 'started').exists()
            else:
                assert result == 1 and sha(target) == old_hash
                if scenario == 'startup_failure':
                    wait_for(lambda: target.with_suffix('.restarted').exists())
                    assert 'original restored' in (stage / 'error.txt').read_text(encoding='utf-8')
        print(f'{scenario}: PASS', flush=True)
    finally:
        for process in (worker, parent):
            if process.poll() is None: process.kill()
            process.wait(timeout=10)
print('Artifacts: ' + str(root), flush=True)
