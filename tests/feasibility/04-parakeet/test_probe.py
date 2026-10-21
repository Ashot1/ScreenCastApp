"""Exercise real child lifecycle and scratch cleanup, independently of ASR."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

PROBE = Path(__file__).with_name('probe.py')


class ProbeTests(unittest.TestCase):
    def invoke(self, child, cancel=None, race=False):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            audio = root / 'fixture.wav'
            audio.write_bytes(b'public fixture')
            scratch = root / 'scratch'
            scratch.mkdir()
            metrics = root / 'metrics.json'
            launcher = [sys.executable, str(PROBE)]
            if race:
                # Delay after a real nonblocking wait so the real child exits before signal.
                bootstrap = '''import os,time,runpy,sys
real = os.wait4
def delayed(pid, flags):
    result = real(pid, flags)
    if flags and result[0] == 0:
        time.sleep(0.3)
    return result
os.wait4 = delayed
path = sys.argv.pop(1)
runpy.run_path(path, run_name="__main__")
'''
                launcher = [sys.executable, '-c', bootstrap, str(PROBE)]
            args = launcher + ['--audio', str(audio),
                    '--metrics', str(metrics), '--work-root', str(scratch)]
            if cancel is not None:
                args += ['--cancel-after', str(cancel)]
            result = subprocess.run(args + ['--', sys.executable, '-c', child,
                                            '{audio}', '{output}'],
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(list(scratch.iterdir()), [])
            self.assertNotIn('private-marker', result.stdout + result.stderr + metrics.read_text())
            self.assertTrue(audio.exists(), 'The original public fixture is not owned scratch')
            return json.loads(metrics.read_text())

    def test_success_removes_owned_audio_and_result_without_logging_text(self):
        result = self.invoke('import json,sys; print("private-marker"); '
                             'json.dump({"text":"private-marker"},open(sys.argv[2],"w"))')
        self.assertEqual(result['status'], 'success')
        self.assertTrue(result['has_text'])
        self.assertTrue(result['worker_reaped'])

    def test_error_removes_worker_created_partial_output(self):
        result = self.invoke('import sys; open(sys.argv[2],"w").write("private-marker"); '
                             'sys.exit(7)')
        self.assertEqual(result['status'], 'error')
        self.assertEqual(result['exit_code'], 7)
        self.assertTrue(result['worker_reaped'])

    def test_malformed_worker_text_is_reported_after_cleanup(self):
        result = self.invoke('import json,sys; json.dump({"text":None},open(sys.argv[2],"w"))')
        self.assertEqual(result["status"], "invalid-output")
        self.assertFalse(result["has_text"])
        self.assertTrue(result["worker_reaped"])

    def test_worker_exit_between_wait_and_signal_preserves_metrics(self):
        result = self.invoke('import json,sys; json.dump({"text":""},open(sys.argv[2],"w"))',
                             cancel=0, race=True)
        self.assertTrue(result["worker_reaped"])

    def test_python_worker_tmpdir_is_inside_operation(self):
        result = self.invoke('import json,sys,tempfile; from pathlib import Path; '
                             'assert Path(tempfile.gettempdir()).is_relative_to(Path(sys.argv[1]).parent); '
                             'Path(tempfile.gettempdir(),"derived.raw").write_bytes(b"audio"); '
                             'json.dump({"text":""},open(sys.argv[2],"w"))')
        self.assertEqual(result["status"], "success")
        self.assertFalse(result["has_text"])

    def test_cancellation_waits_for_signal_resistant_worker_exit(self):
        result = self.invoke('import signal,time,tempfile; from pathlib import Path; '
                             'signal.signal(signal.SIGTERM,signal.SIG_IGN); '
                             'Path(tempfile.gettempdir(),"fluidaudio-streaming-test.raw").write_bytes(b"public"); '
                             'time.sleep(30)', cancel=0.3)
        self.assertEqual(result['status'], 'cancelled')
        self.assertEqual(result['exit_code'], -9)
        self.assertTrue(result['worker_reaped'])
        self.assertGreaterEqual(result['cancel_to_exit_s'], 0.2)
        self.assertEqual(result['derived_audio_files_seen'], 1)


if __name__ == '__main__':
    unittest.main()
