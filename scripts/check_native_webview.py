"""Run a native WebView lifecycle smoke check with a deadline and success marker."""
import argparse
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile


def stop(process):
    if os.name == 'nt':
        try:
            subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
        except (OSError, subprocess.TimeoutExpired):
            pass
        finally:
            if process.poll() is None:
                process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait(timeout=5)


def check(command, timeout=45):
    # File capture cannot hang on inherited pipes held by WebKit child processes.
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                   start_new_session=os.name != 'nt')
        timed_out = False
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            stop(process)
        except BaseException:
            stop(process)
            raise
        finally:
            output.seek(0)
            log = output.read().decode('utf-8', errors='replace')
            print(log, end='' if log.endswith('\n') else '\n', flush=True)

    if timed_out:
        raise RuntimeError(f'WebView smoke timed out after {timeout:g} seconds')
    if process.returncode != 0:
        raise RuntimeError(f'WebView smoke exited with code {process.returncode}')
    if 'WEBVIEW_SMOKE_FAILED' in log:
        raise RuntimeError('WebView reported a smoke failure')
    if not re.search(r'^WEBVIEW_SMOKE_OK(?::|\s*$)', log, re.MULTILINE):
        raise RuntimeError('WebView exited without the WEBVIEW_SMOKE_OK marker')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('executable', type=Path)
    parser.add_argument('--timeout', type=float, default=45)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    try:
        check([str(args.executable.resolve()), '--smoke'], args.timeout)
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error


if __name__ == '__main__':
    main()
