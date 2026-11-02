#!/usr/bin/env python3
"""PTY characterization for the interactive stdin-script flow."""

import os
import pty
import select
import sys
import time
import tempfile


def read_until(fd, marker, timeout=5):
    data = bytearray()
    deadline = time.monotonic() + timeout
    while marker not in data:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError(f"timed out waiting for {marker!r}; got {bytes(data)!r}")
        ready, _, _ = select.select([fd], [], [], remaining)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 4096)
        except OSError:
            break
        if not chunk:
            break
        data.extend(chunk)
    return bytes(data)


def main():
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} PATH_TO_FWS")

    with tempfile.TemporaryDirectory() as directory:
        marker = os.path.join(directory, "executed")
        pid, fd = pty.fork()
        if pid == 0:
            os.execv(sys.argv[1], [sys.argv[1]])

        try:
            prompt = read_until(fd, b"Paste Bash script, then press Ctrl-D to run: ")
            assert b"Paste Bash script" in prompt, prompt
            script = (
                f"touch {marker}\n"
                "printf 'MULTI\\n'\n"
                "IFS= read -r line\n"
                "printf 'INPUT<%s>\\n' \"$line\"\n"
            ).encode()
            os.write(fd, script)
            time.sleep(0.15)
            assert not os.path.exists(marker), "script executed before terminal EOF"

            os.write(fd, b"\x04")
            result = read_until(fd, b"MULTI\r\n")
            assert b"MULTI\r\n" in result, result
            os.write(fd, b"terminal input\n")
            result += read_until(fd, b"INPUT<terminal input>\r\n")
            assert b"INPUT<terminal input>\r\n" in result, result
            _, status = os.waitpid(pid, 0)
            assert os.waitstatus_to_exitcode(status) == 0, status
        finally:
            try:
                os.close(fd)
            except OSError:
                pass


if __name__ == "__main__":
    main()
