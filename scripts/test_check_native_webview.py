"""Exercise the native smoke harness against real child processes."""
import contextlib
import io
import sys
import unittest

from check_native_webview import check


class NativeWebViewCheckTests(unittest.TestCase):
    def run_child(self, source, timeout=5):
        return check([sys.executable, '-c', source], timeout)

    def test_success_requires_completed_check(self):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.run_child("print('WEBVIEW_SMOKE_OK: lifecycle')")
        self.assertIn('WEBVIEW_SMOKE_OK: lifecycle', output.getvalue())

    def test_zero_exit_without_marker_fails(self):
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, 'without.*marker'):
                self.run_child("print('application closed')")

    def test_failure_exit_overrides_success_marker(self):
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, 'code 7'):
                self.run_child("print('WEBVIEW_SMOKE_OK'); raise SystemExit(7)")

    def test_failure_marker_overrides_success_marker(self):
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, 'reported a smoke failure'):
                self.run_child("print('WEBVIEW_SMOKE_OK\\nWEBVIEW_SMOKE_FAILED: failed')")

    def test_deadline_preserves_diagnostics(self):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            with self.assertRaisesRegex(RuntimeError, 'timed out'):
                self.run_child("import time; print('before timeout', flush=True); time.sleep(60)", 1)
        self.assertIn('before timeout', output.getvalue())


if __name__ == '__main__':
    unittest.main()
