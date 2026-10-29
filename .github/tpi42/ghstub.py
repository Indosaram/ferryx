import functools, http.server, ssl, sys
# Loopback HTTPS stand-in for github.com release downloads (SHA256SUMS only).
root, cert, key, port = sys.argv[1:5]
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=root)
srv = http.server.ThreadingHTTPServer(("127.0.0.1", int(port)), handler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(cert, key)
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
print(f"serving {root} on https://127.0.0.1:{port}", flush=True)
srv.serve_forever()
