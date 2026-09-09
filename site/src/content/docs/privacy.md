---
title: Ferryx Privacy Policy
description: How Ferryx processes local workspace data, terminal sessions, browser data, and optional remote connections.
lastUpdated: 2026-09-09
prev: false
next: false
---

**Effective date: September 9, 2026**

**Publisher: Project Maho**

Ferryx is a desktop terminal and workspace manager. This policy covers the
Ferryx application, its remote web client, and the Ferryx website. Features
available to you may differ by platform, version, and settings.

## Data used on your devices

Ferryx processes the information needed to run and restore your workspace:

- **Workspace settings:** project locations, Git worktree and branch information,
  tab and pane layouts, terminal preferences, and session identifiers.
- **Terminal sessions:** commands you enter, process information, working
  directories, and terminal output. Output may contain personal information,
  source code, or secrets printed by the programs you run.
- **Agent sessions:** session identifiers and locally available conversation
  records used to find, display, or resume supported coding-agent sessions.
- **Browser data:** pages you visit in embedded browser tabs, cookies and website
  storage, and, when enabled, browsing history such as URLs, titles, and visit
  times.
- **Files and clipboard content:** files you choose to open or attach, and text or
  images you copy, paste, or drop into the application.
- **Diagnostics:** local logs and error details used to troubleshoot application,
  terminal, or connection problems.

Settings and saved state are stored in application files or local browser
storage. Recent terminal output is also buffered for reconnection. A background
process can keep terminal sessions running after you close the application
window. Closing the window is not the same as deleting a session or its data.

## Remote connections and external tools

Local terminal use does not require a Project Maho account. Network features
process additional data when you use them:

- **Remote web access:** a paired browser exchanges authentication information,
  workspace and session metadata, terminal output, and input with the Ferryx host
  you connect to. People with access to an authorized client may see the sessions
  exposed to that client.
- **SSH workspaces:** commands, terminal input and output, and files you transfer
  are exchanged with the remote host you select. That host and its administrator
  may retain their own files, shell history, and logs.
- **Coding agents and AI services:** agents or provider tools you launch may send
  prompts, selected files, conversation context, tool output, and attachments to
  their configured services. Their account settings and privacy policies govern
  that processing; running an agent inside Ferryx does not make it offline.
- **Embedded websites:** visiting a website sends requests to that website. It
  can receive your IP address and browser information and use cookies or other
  website storage under its own policy.

These connections are used to provide the requested terminal, browsing, remote,
or agent functionality. Do not send information to a host, website, or provider
that you do not intend to receive it.

## Notifications

When enabled, notifications can display workspace or agent activity through your
operating system or browser. Notification text may be visible on a lock screen
or to other people using your device. You can control notification permissions
in the operating system or browser.

## Updates and this website

Non-Store builds can contact GitHub-hosted release endpoints to check for or
download updates. Microsoft Store installations use Microsoft's update
mechanism. These services receive ordinary network request information, such
as your IP address, and process it under their own policies.

The Ferryx website is hosted on GitHub Pages. GitHub may process technical
request information when you visit it. The website uses local storage for
preferences such as your selected theme; some pages load fonts from Google
Fonts. Those font requests also disclose network request information to Google.

- [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)
- [Microsoft's privacy statement](https://privacy.microsoft.com/privacystatement)
- [Google's privacy policy](https://policies.google.com/privacy)

## Retention and your choices

Local settings, saved state, browser data, and logs can remain on your device
until you remove or replace them. Terminal replay buffers are bounded rather
than a permanent archive, but shells, agents, and remote hosts may keep separate
histories. There is no single retention period covering all these tools.

You can:

- Disable browsing-history recording and clear Ferryx's browsing-history list.
- Turn off remote access when you do not want clients connecting to your host.
- Revoke notification permissions in your operating system or browser.
- Clear site data in the browser used for the Ferryx website or remote client.
- Remove local application data you no longer need after ending the relevant
  sessions and backing up anything you want to keep.
- Use the deletion controls of the websites, AI providers, and remote hosts you
  connect to for information stored by those services.

Clearing Ferryx's browsing-history list does not clear website cookies. Removing
a workspace from the interface does not necessarily delete its repository,
agent history, or files. Uninstalling the application may leave configuration,
logs, or data created by other programs. Contact us if you need help identifying
which local data to remove.

## Support and privacy requests

Project Maho receives information you choose to share when you contact us,
including your message and any diagnostic material you attach. We use that
information to respond to your request and investigate the reported issue.

For privacy questions or help accessing, correcting, or deleting information,
contact the Ferryx maintainers through the
[Ferryx support page on GitHub](https://github.com/Indosaram/ferryx/issues).
GitHub issues are public: do not include credentials, private source code, or
other sensitive personal information. If your request needs private details,
ask for a private contact method before sharing them.

Requests concerning data held by an external service should also be directed
to that service. Its own retention rules and privacy policy apply.

## Changes to this policy

We will publish changes on this page and update the effective date above.
