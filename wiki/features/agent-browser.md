# Let an agent use the browser

An agent running in a terminal tab of a workspace can drive that workspace's browser with no setup. Run
`termenbox-server browser <action>` from the agent's shell: `open`, `navigate <url>`, `text`, `click <x> <y>`,
`type <text>`, `key <name>`, `scroll <x> <y> <dx> <dy>` and `release`. Every tab already carries the variables the
command reads. If the workspace has no browser, the first action opens a browser tab and tells you which agent did. An agent cannot start the first download: until you have opened a browser tab once, its command answers that the owner must do so.

The command is a shell command because every agent has a shell tool. Only a process in a tab of the workspace can use
it; any other process is refused and the browser stays as it was. Addresses on the server's own network are refused
unless you allowed that host.

The tab shows the agent's name while it drives. You can pause it (its actions wait while you type in the page) and stop it
(its actions end and the page stays). When the agent hits a login or a CAPTCHA it raises a handoff
(`termenbox-server handoff`); the card opens the tab, you solve the step and answer "Done".

The server keeps its own copy of the browser and never uses one you installed. It downloads that copy once, when you first open a browser tab, from a place
you pin. The server pins the build below by default, so a browser tab works with no setting. To pin another build, set both lines in the server's settings:

```
TERMENBOX_BROWSER_URL=https://storage.googleapis.com/chrome-for-testing-public/157.0.8103.78/linux64/chrome-headless-shell-linux64.zip
TERMENBOX_BROWSER_SHA256=f90329720fb61e5a76da9b6b509b6ad12a484c9c5df0359403e935177c6d153a
```

These pin the Chrome for Testing headless shell 157.0.8103.78 for 64-bit Intel Linux. The server refuses the download if its checksum
differs, unpacks a zip into its own folder (no link, no path outside it, at most 2000 files and 1 GiB), and does not fetch
again after a restart. To move to another version, change both lines together; one line alone is ignored and the default stays.
