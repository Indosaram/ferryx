---
title: Introduction
description: Overview of Ferryx terminal workspace.
---

Ferryx is an ultra-lightweight, native AI workspace built on Tauri v2, powered by a native libghostty terminal engine with wgpu GPU rendering and a fast Rust pseudoterminal daemon. It gives you split-pane tiling, an embedded web companion for browser tabs alongside your terminals, and authenticated mobile remote control so you can reach your sessions from any paired browser.

## Native Ghostty Terminal Engine

Desktop terminal surfaces render directly through a native `libghostty` and `wgpu` graphics pipeline, delivering hardware-accelerated glyph rendering, precise ANSI parsing, and low-latency throughput without web-layer terminal overhead.

## Split-Pane Tiling

Arrange terminal panes in flexible vertical and horizontal splits within a workspace, with pointer-driven resizing and smooth layout transitions.

## Embedded Web Companion

Open browser tabs directly alongside your terminal tabs in the same workspace, backed by the native webview.

## Mobile Remote Control

Pair a mobile device with your running Ferryx daemon to view and steer terminal sessions remotely over an authenticated WebSocket connection using a lightweight browser xterm.js interface.

See the [keyboard shortcuts](/docs/shortcuts/) reference for the in-app bindings.
