"""Local experiment supervisor. No transcription or audio is emitted in metrics."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time


def signal_worker(pid, value):
    # The worker can exit after wait4's check. wait4 remains the only reaper.
    try:
        os.kill(pid, value)
    except ProcessLookupError:
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--audio', type=Path, required=True)
    parser.add_argument('--metrics', type=Path, required=True)
    parser.add_argument('--work-root', type=Path)
    parser.add_argument('--cancel-after', type=float)
    parser.add_argument('--after-ready', action='store_true')
    parser.add_argument('--timeout', type=float, default=180)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('worker command required')
    started = time.monotonic()
    cancel_at = None
    ready_at = None
    status = 'success'
    with tempfile.TemporaryDirectory(prefix='parakeet-', dir=args.work_root) as work:
        work = Path(work)
        audio, output, ready = work / 'audio.wav', work / 'result.json', work / 'ready'
        shutil.copyfile(args.audio, audio)
        substitutions = {'{audio}': str(audio), '{output}': str(output), '{ready}': str(ready)}
        # The completed input is copied before spawning. No capture or cloud path exists.
        derived_seen = set()
        child = subprocess.Popen([substitutions.get(x, x) for x in command],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 cwd=work, env={**os.environ, 'TMPDIR': str(work) + '/'})
        try:
            while True:
                derived_seen.update(x.name for x in work.glob('fluidaudio-streaming-*.raw'))
                pid, exit_status, usage = os.wait4(child.pid, os.WNOHANG)
                if pid:
                    child.returncode = os.waitstatus_to_exitcode(exit_status)
                    break
                now = time.monotonic()
                if ready_at is None and ready.exists():
                    ready_at = now
                origin = ready_at if args.after_ready else started
                should_cancel = (args.cancel_after is not None and origin is not None
                                 and now - origin >= args.cancel_after)
                if cancel_at is None and (should_cancel or now - started >= args.timeout):
                    status = 'cancelled' if should_cancel else 'timeout'
                    cancel_at = now
                    signal_worker(child.pid, signal.SIGTERM)
                elif cancel_at is not None and now - cancel_at >= 0.25:
                    signal_worker(child.pid, signal.SIGKILL)
                    _, exit_status, usage = os.wait4(child.pid, 0)
                    child.returncode = os.waitstatus_to_exitcode(exit_status)
                    break
                time.sleep(0.01)
        finally:
            # A request to stop is not release: reap the actual process before cleanup.
            if child.returncode is None:
                try:
                    signal_worker(child.pid, signal.SIGKILL)
                finally:
                    child.wait()
        finished = time.monotonic()
        text = ''
        inference_s = None
        if status == 'success':
            if child.returncode != 0:
                status = 'error'
            else:
                try:
                    result = json.loads(output.read_text())
                    value = result['text']
                    if not isinstance(value, str):
                        raise ValueError('invalid text')
                    text = value
                    inference_s = result.get('inference_s')
                except (OSError, ValueError, KeyError, TypeError):
                    status = 'invalid-output'
        metrics = dict(status=status, exit_code=child.returncode,
                       worker_reaped=True, has_text=bool(text.strip()),
                       text_characters=len(text), elapsed_s=finished-started,
                       inference_s=inference_s,
                       derived_audio_files_seen=len(derived_seen),
                       cpu_s=usage.ru_utime+usage.ru_stime,
                       peak_process_rss_mib=usage.ru_maxrss/1024/1024,
                       inference_ready=ready_at is not None,
                       cancel_to_exit_s=None if cancel_at is None else finished-cancel_at)
    metrics['owned_scratch_removed'] = not work.exists()
    args.metrics.write_text(json.dumps(metrics, indent=2) + '\n')


if __name__ == '__main__':
    main()
