"""Isolated password-only SSH server driving the real OpenSSH/Rust surfaces.

Run with asyncssh installed and target/debug/examples/ssh_password_fixture built.
Never reads user SSH config, changes user accounts, or touches an existing daemon.
"""
import asyncio
import json
import os
from pathlib import Path
import secrets
import tempfile
import asyncssh


async def main():
    with tempfile.TemporaryDirectory(prefix="fxp-", dir="/tmp") as directory:
        root = Path(directory).resolve()
        directory = str(root)
        password = secrets.token_urlsafe(32)
        key = asyncssh.generate_private_key("ssh-ed25519")
        connections = set()
        class Server(asyncssh.SSHServer):
            def connection_made(self, connection):
                self.connection = connection
                connections.add(connection)
            def connection_lost(self, exc): connections.discard(self.connection)
            def begin_auth(self, username): return True
            def password_auth_supported(self): return True
            def validate_password(self, username, supplied):
                return username == "fixture" and secrets.compare_digest(supplied, password)

        async def process(session):
            if session.command == "FERRYX_FIXTURE_DISCONNECT":
                session.exit(0)
                for connection in list(connections): connection.abort()
                return
            env = dict(os.environ, HOME=directory, TMPDIR=directory, SHELL="/bin/sh")
            child = await asyncio.create_subprocess_exec(
                "/bin/sh", "-c", session.command, cwd=directory, env=env,
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE)
            async def feed():
                try:
                    while data := await session.stdin.read(65536):
                        child.stdin.write(data)
                        await child.stdin.drain()
                except (BrokenPipeError, ConnectionResetError):
                    pass
                finally:
                    child.stdin.close()
            async def drain(source, destination):
                while data := await source.read(65536):
                    destination.write(data)
            tasks = [asyncio.create_task(feed()), asyncio.create_task(drain(child.stdout, session.stdout)), asyncio.create_task(drain(child.stderr, session.stderr))]
            try:
                code = await child.wait()
                await asyncio.gather(*tasks[1:])
                session.exit(code)
            finally:
                tasks[0].cancel()
                await asyncio.gather(tasks[0], return_exceptions=True)
                if child.returncode is None:
                    child.terminate()
                    await child.wait()

        server = await asyncssh.create_server(Server, "127.0.0.1", 0,
            server_host_keys=[key], process_factory=process, encoding=None)
        port = server.get_port()
        trust = root / "known_hosts"
        trust.write_bytes(f"[127.0.0.1]:{port} ".encode() + key.export_public_key())
        bindir = root / "bin"
        bindir.mkdir()
        wrapper = bindir / "ssh"
        wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile="{trust}" -o GlobalKnownHostsFile=/dev/null "$@"\n')
        wrapper.chmod(0o700)
        env = dict(os.environ, PATH=f"{bindir}:" + os.environ["PATH"])
        child = await asyncio.create_subprocess_exec("src-tauri/target/debug/examples/ssh_password_fixture", directory, str(port), env=env, stdin=asyncio.subprocess.PIPE)
        # The secret crosses stdin only; never argv, environment, or disk.
        child.stdin.write((password + "\n").encode())
        await child.stdin.drain()
        child.stdin.close()
        try:
            code = await asyncio.wait_for(child.wait(), 90)
            if code: raise RuntimeError(f"Rust fixture exited {code}")
            print(json.dumps({"passwordOnlySsh": "passed", "port": port}))
        finally:
            if child.returncode is None:
                child.terminate()
                await child.wait()
            server.close()
            await server.wait_closed()


asyncio.run(main())
