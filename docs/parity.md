# Parity with Copper for Mac

The Mac is the reference. This table lists every user-visible Mac feature and
whether Copper for Linux (`copper-linux`, GTK) and Copper for Windows (this
repo) have it. It is the to-do list for both ports: when a Mac change lands,
add or update its row here (the Mac repo's CONTRIBUTING.md asks for that).

**Audited against** `copper-browser/copper` `fork` @ **34649ab** (2026-10-29);
statuses as of copper-linux 0.6.1, copper-windows 0.3.0 (2026-10-30). Update the commit when you re-audit.

Status: **yes** · **part** (partial, note says what's missing) · **no** ·
**mac** (can't exist off the Mac; note says why). *Shared* names the
toolkit-free crate in copper-linux that holds (or should hold) the logic; the
platform shells only draw it. "L" = Linux, "W" = Windows.

| Area | Feature | Linux | Windows | Shared | Note |
|---|---|---|---|---|---|
| Tabs | Tabs in a column / across the top, Ctrl+Shift+S toggles | yes | yes | shell-core | |
| Tabs | Hide the column (⌘S) + reveal on edge hover | part | part | platform | both fold on Ctrl+S, no hover reveal |
| Tabs | Resize the column by dragging its edge | no | no | core `sidebar_width` | |
| Tabs | Live tab pill becomes the address field (top row) | no | yes | platform | |
| Tabs | Pinned tabs (favourites grid), pin/unpin, Change Letter | yes | yes | core | L has no Change Letter UI |
| Tabs | Drag to reorder pins / reorder in the top row | yes | no | shell-core `drop_tab` | |
| Tabs | Drag a row onto favourites to pin it | yes | yes | shell-core `drop_on_pins` | |
| Tabs | Saved / Today sections, seam, Save/Unsave, ⌘W puts a Saved tab down | yes | yes | core, shell-core | |
| Tabs | Archive Today after 12/24/48 h | yes | yes | shell-core `archive` | W sweep (`start_timers`) + Settings › Tabs row |
| Tabs | Drag across the seam with a landing marker; "Drop here to save" | yes | part | shell-core `sidedrop` | W no empty-Saved slot |
| Tabs | Folders drawn in the column, fold/unfold | yes | yes | shell-core `folders::plan` | L draws flat rows |
| Tabs | Drag a tab into / out of a folder, Remove from Folder | yes | yes | shell-core `sidedrop`, `groups::take_out` | |
| Tabs | New folder from tab (⌃⇧G), Move to folder, folder menu (rename/colour/ungroup/close all) | no | no | shell-core `groups` | rename/dissolve need shared fns |
| Tabs | Tab menu: Duplicate, Copy Address, Close, Close Others | yes | yes | shell-core | |
| Tabs | Middle-click closes a row; middle/Ctrl-click link opens behind | yes | yes | platform | |
| Tabs | Mouse thumb buttons / two-finger swipe back-forward | part | part | platform | engine defaults over the page only |
| Tabs | ⌘1–9 nth tab, ⌃Tab / Ctrl+PgUp/PgDn | yes | yes | shell-core | |
| Tabs | ⌃Tab in most-recent order (setting) + hold strip | part | part | shell-core `recent`, `switch_tab` | order yes on both, no hold strip |
| Tabs | Reopen closed ⌘⇧T (restores place + folder) | yes | yes | core | |
| Tabs | Recently Closed list | part | part | core `closed_tabs` | Ctrl+T / palette section on both |
| Tabs | Clear tabs ⌘⇧K + Undo; "↓ Clear" seam button | yes | yes | shell-core `clear` | L no seam button |
| Tabs | Tab sleep (idle / memory), site marks, audio badge | yes | yes | shell-core `sleep` | W also freezes hidden WebView2s |
| Tabs | Hot-tab CPU reading, sleep hot tabs sooner | no | no | platform sampling | |
| Tabs | Wake cover (last picture while a page rebuilds) | no | yes | shell-core `cover` | L never calls `on_cover` |
| Tabs | Split view ⌘⇧D (two panes) | no | no | shell-core | |
| Tabs | Multiple windows ⌘N, per-window space | no | no | shell-core | single window on both |
| Tabs | Full screen | yes | yes | platform | L F11, W F11 |
| Tabs | Drop a link / text on the column opens a tab | no | no | platform | |
| Spaces | Spaces: header, chips at the foot, click/drag reorder, + new | yes | yes | core, shell-core | |
| Spaces | Space menu: New Tab in, Move Tab Here, Move L/R, Colour, Profile, Delete | part | yes | shell-core | L lacks New Tab in / Move Here / Colour / Profile |
| Spaces | Delete space sheet (close or move tabs) | yes | yes | shell-core | |
| Spaces | 10 named colours, column tint from the space | yes | yes | shell-core `colour`, `Tint` | |
| Spaces | Space icon: emoji + symbol | part | yes | core | L emoji only |
| Spaces | Profiles per space (own cookie jar, named / shared) | part | yes | shell-core | L own-jar switch only |
| Spaces | Gradient theme | part | part | shell-core `Theme` | L drawn not editable, W flattened to one colour |
| Spaces | Picture backdrop, intensity / grain / blur sliders, animated scene | no | no | core `SpaceTheme` | needs a web view host for the scene |
| Spaces | Settings › Spaces page with live preview | part | part | platform | L editor dialog, W no preview |
| Spaces | Switch slide / ground blend; swipe between spaces | no | part | platform | W slide only |
| Spaces | Warm other spaces' pages after launch | no | no | shell-core | |
| Spaces | ⌃⌥←/→ prev/next; ⌃1–9 nth space; ⌃N new space | yes | yes | shell-core | Alt+1–9 / Alt+N on both |
| Field | One field: Open Tabs → History → Bookmarks → Search, inline completion, Tab between sections | yes | yes | shell-core `palette/complete` | W has its own `rank.rs`; unify |
| Field | ⌘T launcher card (Recent Tabs + Recently Closed) | part | yes | shell-core | L no Closed section |
| Field | Google search suggestions while typing | no | no | shell-core | |
| Field | ⌘K palette: tabs, other spaces, bookmarks, canvases, commands | part | yes | shell-core | L no Canvases, no commands on empty |
| Field | ⌘K held walks rows, release lands; page's own ⌘K wins (Linear, Slack) | no | part | platform, pagejs | W walk yes, PageFirst no |
| Field | Copy address ⇧⌘C, Paste and Go ⇧⌘V | part | part | platform | commands exist, keys missing |
| Panels | History (by day, search, remove, clear) ⌘Y | yes | yes | core | L Ctrl+Y, W Ctrl+H |
| Panels | Bookmarks panel; Bookmark this page ⇧⌘B | yes | yes | core | L Ctrl+D |
| Panels | Downloads panel ⇧⌘J, door ring, Show in folder, cancel/retry | yes | yes | shell-core `downloads` | |
| Panels | Ask where to save each file | no | yes | core `downloads.ask` | |
| Page | Find in page ⌘F / ⌘G / ⇧⌘G | yes | yes | platform + engine | |
| Page | Reading mode ⌥⌘R | yes | no | pagejs `tab::reader` | W key bound, no command |
| Page | Hide element ⇧⌘H + undo, per site; Hidden here ⇧⌘U | yes | yes | shell-core `curtain` | L no ⇧⌘U key |
| Page | Ad blocker, off for this site | yes | yes | shield, shell-core | |
| Page | Private tabs ⇧⌘N | yes | yes | shell-core `private` | |
| Page | Float video (PiP) ⇧⌘P | no | no | platform | WebKitGTK has no PiP |
| Page | Stop sound in tab ⇧⌘M | yes | yes | engine `set_muted` | L engine has mute, no action |
| Page | Copper's image context menu | part | yes | platform | L WebKitGTK stock menu |
| Page | Pointer lock granted silently, Esc hint | part | yes | shell-core | L asks every time |
| Page | Web Inspector with the element picker armed ⇧⌘I | part | part | engine | picker not armed on either |
| Page | Camera / mic / location / notification prompts remembered per site; Forget choices | yes | yes | shell-core `ask_permission` | shared `permit.rs`; W still its own copy |
| Page | Camera / mic asks wait their turn, never refused for coming second; a call's mic + camera asked once, one Allow settles every waiting page of that site; Show for a tab you aren't on | part | part | shell-core `permit` | W refuses a second site while one waits (joins one getUserMedia's two asks); L one dialog per ask |
| Page | Allow kept per site and device; Don't allow never kept (holds until the page reloads, Allow and reload for 8 s); older noes dropped; Permissions API says granted for an allowed site | no | no | shell-core `permit` | both keep a refusal per kind (`capture.{host}\|camera`) |
| Page | The OS refusing the camera / mic said in the bar (open its settings; Reload once allowed); a blocked camera never costs a call its mic | no | part | platform | W does this for location and notifications only |
| Page | A call keeps hearing you in a background tab or a covered window (its re-asks answered at once, never parked as hidden); the call on screen takes back a mic another tab paused (Use it here) | no | no | platform | engine-specific; not audited on either port |
| Page | Share a single tab from getDisplayMedia: Copper's picker (this space's tabs, then other spaces', then A window… / Entire screen…), the tab alone at up to 30 fps, kept painting while you are elsewhere; only the asking frame gets it; no tab audio | no | no | platform | L portal (screen / window), W WebView2's own picker |
| Page | Sharing *tab* with *site* · Stop at the foot of the window; closing the tab, leaving the call or the page's stop end it (`ended`) | no | no | platform | |
| Page | Bad certificate: ask once, local hosts trusted | no | part | engine, shell-core | L load just fails |
| Page | JS dialogs, HTTP auth, file chooser, crashed-tab page | yes | yes | engine | |
| Page | Print ⌘P | yes | yes | platform | L print dialog, W WebView2's own |
| Page | Zoom ⌘+/−/0; zoom remembered per site | yes | yes | shell-core `zoom` | L per-site memory missing; W per site from shell-core (`pages.rs`) |
| Page | Light / Dark / System | yes | yes | core | |
| App | Announcement toast, length-scaled timing | yes | yes | platform | shared `view::toast_time` |
| App | Send Feedback (GitHub issue) | no | yes | platform | |
| App | Update check (6 h + Check now) | yes | yes | shell-core `updates` | |
| App | In-app update download + install + relaunch; update pill in the column | part | part | shell-core + installer | both notify + link only |
| App | Links from other apps land in the front window; default browser; one instance | yes | yes | platform | |
| App | Session never overwritten by a half-built launch | yes | yes | core `store` | |
| App | Restore previous session (command) | no | no | core has the file | |
| App | Crash records, `copper crashes`, About › Diagnostics | part | part | shell-core `crash` | web-process faults only, no panic capture |
| App | Welcome / onboarding | no | yes | platform | |
| App | Keyboard shortcuts list in About | no | part | platform | W lists a dead Reading mode key |
| App | Launch watchdog, traffic lights, column scroll bounce, menu-bar menus | mac | mac | — | AppKit / SwiftUI |
| Cloud | Settings › Cloud: Connect (link code / join link / pairing code), by hand, pinned cert, TOFU | yes | yes | copper-cloud `link`, `tls` | |
| Cloud | Sign in / Create account; Continue with Google (PKCE); password form folded by server flag | yes | yes | copper-cloud `sso`, shell-core `cloud` | |
| Cloud | `copper://cloud/join` deep link starts SSO; typed join link offers "Join Copper Cloud — HOST" | yes | part | shell-core `cloud/join` | W prefills, doesn't start SSO |
| Cloud | Pair another device (code, copy, countdown, revoke); pair from a `cp_` code | yes | yes | copper-cloud | |
| Cloud | Sync switches: spaces, settings, bookmarks, tabs, history; Personal canvas | yes | yes | copper-cloud `sync` | |
| Cloud | Spaces + kept tabs + folders sync; one pin per site; pins re-key | yes | yes | copper-cloud `merge`, shell-core `adapter` | |
| Cloud | Intents (overwrite only what changed here), pull-first | yes | yes | copper-cloud `intent` | |
| Cloud | Brake (>10 unfiled/removed asks) as a column card | part | part | copper-cloud `held` | modal question, not a card |
| Cloud | Settings sync (9-key allowlist), bookmarks merge | yes | yes | core `CLOUD_SETTINGS_KEYS` | |
| Cloud | "On your other devices" card | yes | yes | copper-cloud | |
| Cloud | History push/pull; Clear History / Forget page delete on the cloud too | yes | yes | shell-core `history_cleared/forgot` | fixed in copper-linux 0.6.0 |
| Cloud | Settings › Cloud › Delete history on cloud (span + site) | yes | yes | copper-cloud `delete`, shell-core `cloud::eraser` | |
| Cloud | Sign out, rename device, log card, SSE reconnect, 401 signs out | yes | yes | copper-cloud | |
| Cloud | Model keys from the cloud, shown as the source | part | yes | copper-mcp `intelligence::merge` | L has no Intelligence page |
| Addons | Agent sidebar ⌘E filled from the cloud; width drag; hidden slot keeps running | yes | yes | core `addons` | |
| Addons | Fetch / verify / stage; check at launch, 6 h, update check | yes | yes | copper-cloud `addons` | |
| Addons | Install offer in the update pill; ⌘K "Install <name>"; Remind Me Later | part | part | platform | toast instead of pill on both |
| Addons | Fallback to the agent pane after load failure / 3 crashes, reason shown | part | part | core `addons/state` | reload on crash only |
| Addons | Built-in agent when no addon | no | yes | shell-core `chat` | L placeholder text |
| Addons | Settings › Extensions › Addons (list, use, remove, check); dev folder + Reload | yes | part | core | W Remove doesn't ask; no Reload button |
| Addons | Bridge `copper.*`: info, settings, identity, storage, fetch, oauth, openUrl, page | yes | yes | core `addons/bridge` | bridge.js byte-identical |
| Addons | Bridge `copper.agent.*` + `agent` event | no | yes | core bridge, shell-core chat | L "no native agent yet" |
| Addons | Bridge `copper.voice.*` + `voice` event, ⌃⇧D into the addon | no | no | needs Voice | shared `run()` answers `bad_request` as the Mac |
| Addons | `oauth.authorize` in its own window | yes | part | core | W uses a tab |
| Agent links | Settings › Agents › Your agents: per-app cards, token, grants, recent calls, Revoke | no | no | copper-mcp (to add) | |
| Agent links | Outbound link stream (hello / heartbeat / backoff), Jev progress over the link, `copper link` CLI | no | no | copper-mcp (to add) | |
| Passwords | OS store save / fill / forget | yes | yes | copper-creds `store` | L Secret Service, W Credential Manager |
| Passwords | Offer to save after a sign-in that took; offer to update a changed password | yes | yes | copper-creds `offer`, shell-core `watch` | |
| Passwords | Fill only on click; list under the box; merged sources | yes | yes | shell-core `vault` | |
| Passwords | Sites never asked (Forget / Forget all); no save/fill in private tabs | yes | yes | shell-core | |
| Passwords | Sign-ins inside iframes | no | no | pagejs relay, shell-core `watch` | relay is main-frame only |
| Passwords | Accounts popup (key door, ⇧⌘Y, search, Fill / Copy / Open) | no | no | shell-core `vault::all_credentials` | data there, no UI |
| Passwords | Two-step code rows (Bitwarden / 1Password TOTP) | yes | yes | copper-creds `totp` | |
| Passwords | Fill two-step codes on their own after a pick (setting) | yes | yes | shell-core `watch` | |
| Passwords | Two-step codes for OS-store logins (Add code) | no | no | copper-creds `store` | |
| Passwords | Suggest strong password; save first, fill new + confirm; offer to save a typed new password | no | no | copper-creds, pagejs `relay.js` | |
| Passwords | Identity / card autofill rows; "Fill addresses and cards" switch | yes | yes | shell-core `view` | L switch missing |
| Passwords | Custom-field rows; most-used usernames on any email box | no | no | shell-core `watch` | |
| Passwords | Bitwarden: connect, lock / unlock / sign out / sync, authenticator + YubiKey code | yes | yes | copper-creds `bitwarden` | |
| Passwords | Bitwarden: email code + new-device code (interactive `bw login`) | part | part | copper-creds `bitwarden` | one-shot login only |
| Passwords | Bitwarden: API-key sign-in; stay unlocked between launches; auto-lock 5/15/60/Never | yes | yes | copper-creds, shell-core `forms::autolock` | L no auto-lock, stay-unlocked off |
| Passwords | 1Password via `op` (app / password / service account), vault for new logins | yes | yes | copper-creds `onepassword` | |
| Passwords | 1Password extension sign-in / app link (native messaging) | mac | mac | — | needs WebExtensions + native messaging |
| Passwords | BotPass source (pair, unlock, fill) | yes | part | copper-botpass | W fills, no pairing card; not on Mac |
| Passwords | Save new passwords to (chooser); Agent access (share everything + per item) | yes | yes | shell-core | |
| Passwords | `browser_sign_in`, `browser_autofill` card / identity / field + CLI | yes | yes | shell-core `forms/mcp` | |
| Passwords | `browser_autofill kind=new-password` | no | no | shell-core `forms/mcp` | needs strong password |
| Passwords | Passwords list: search, add, show, copy, remove; re-auth before show / export | part | part | shell-core `vault` | no Windows Hello / polkit gate |
| Passwords | CSV import / export | yes | yes | copper-import `passwords_csv`, shell-core `forms::csv` | L unwired |
| Passwords | Copper's own passkeys (page authenticator, picker, Settings list) | no | part | pagejs (to add) | W Windows Hello only; L removes WebAuthn |
| Agents | Loopback MCP server, bearer, Origin check, `--mcp-stdio` bridge | yes | yes | copper-mcp | |
| Agents | Settings › Agents: switch, status, port, token, Copy config / stdio config | yes | yes | shell-core `agents` | W port not typed |
| Agents | Copy prompt / Copy /jev / install command; `copper setup` for phi, Claude Code | no | no | shell-core, copper-mcp `cli` | |
| Agents | 26 `browser_*`, 4 `jev_*`, 12 `canvas_*`, `cloud_history_delete` tools | yes | yes | copper-mcp `catalogue` | |
| Agents | `voice_status`, `voice_transcript` | no | no | needs Voice | |
| Agents | Who is calling, per-caller agent tabs, backstage paints, announce each call | yes | yes | copper-mcp, shell-core | |
| Agents | CLI page verbs, `signin`, `autofill`, `cloud-history` | yes | yes | copper-mcp `cli` | |
| Agents | CLI `run --vision/--model/--strategy/--json-run` | no | no | copper-mcp `ultrafast` | |
| Agents | CLI `intelligence`, `claude`, `link`, `bitwarden`, `session`, `crashes`, `listen` | part | part | copper-mcp `Host::control` | `intelligence` works on both; the rest need lanes |
| Agents | Headless mode | yes | no | platform | L weston; low value on W |
| Jev | Jev mode switch, ultrafast loop, page trail | yes | yes | copper-mcp `ultrafast`, pagejs `trail` | |
| Jev | TypeSafe key in Settings (+ Get a key); text model picker | yes | part | copper-core `intelligence` | W no text-model row |
| Jev | Fast mode + strategies (literal / burst / paste / watch / lean) | yes | no | copper-mcp `ultrafast` | shared; no Settings switch, `jevFast` in intelligence.json |
| Jev | What Jev sees (auto / marks / box / pixels), `json` trace | no | no | copper-mcp `ultrafast`, `shot` | |
| Jev | SIGN_IN / AUTOFILL controls inside a run | yes | no | shell-core `jev_extras` | |
| Jev | Text helper over the Claude account | no | no | copper-mcp `claude_ask` | |
| Driving | Drive events; "X is driving" pill + Stop; Stop refuses 30 s | yes | yes | copper-mcp `drive` | |
| Driving | Warm ring inside the page edge; per-tab hand badges | part | yes | platform | L no ring |
| Driving | "Let it back in" (end refusal early) | yes | yes | shell-core `Agents::resume_hand` | |
| Driving | Driver card (who, goal, steps, clock), Details timeline, concurrent band, Driver Timeline ⌥⌘J | no | part | shell-core `chat/transcript` | W card, no Details |
| Agent pane | ⌘E chat with in-process tools, model pill, activity row, markdown, nested Jev, Stop | no | yes | shell-core `chat` | W built-in web UI `agent-ui/`; L unwired |
| Agent pane | Page chip + suggestions, `/jev` toggle, tool rounds (cloud max_turns), error callouts | no | yes | shell-core `chat` | |
| Agent pane | Ask About This Page ⌘⇧E | no | no | platform | Ctrl+Shift+E is New canvas off-Mac |
| Agent pane | Your other MCP servers (`mcp.json`, `server__tool`), Open / Reload | no | no | copper-mcp (client to add) | |
| Agent pane | "Sign in with Claude" empty state | no | no | needs Claude lane | |
| Intelligence | Settings › Intelligence page | part | part | copper-mcp `intelligence` | no Claude lane on either; W no text model / Voice |
| Intelligence | API-key lane: gateway key + address, tier picker, model names | yes | yes | copper-mcp `intelligence` | L env / file only |
| Intelligence | Claude account sign-in (OAuth PKCE, refresh) + Claude lane inference | no | no | copper-cloud or copper-mcp | |
| Intelligence | Check (one ask per lane) | yes | yes | copper-mcp `intelligence::check` | |
| Intelligence | Prompt cache marks; keys from Copper Cloud | yes | yes | shell-core `chat/wire`, copper-cloud | L Jev one-shots only |
| Voice | Dictation into the composer, ⌃⇧D, Listen live transcript, model download | no | no | platform STT + copper-mcp `Transcript` | needs whisper.cpp / ONNX |
| Voice | Phonon-2 Core ML runner | mac | mac | — | Core ML / Neural Engine |
| Canvas | Canvas page `copper://canvas/<id>`, Personal, local canvases, Yjs store, live room, status line, theme | yes | yes | shell-core `canvas`, copper-cloud `canvas` | same canvas-app commit c425198 |
| Canvas | New Canvas ⌃⇧E; Personal Canvas ⌘⇧O | part | yes | shell-core | L New makes a shared canvas when signed in |
| Canvas | ⌘K: Open Canvas · name, New Shared, Share, Join | no | yes | W `rank::canvas_offers` → shell-core | |
| Canvas | Canvas door at the sidebar foot (Personal / Invitations / My / Shared), dot, row menu | no | yes | W `canvas/door.rs`, `copy.rs` → shell-core | biggest L gap |
| Canvas | Rename / Delete / Leave (door + tab menu) | no | yes | W `canvas/ops.rs` → shell-core | |
| Canvas | Canvas mark favicon, "Canvas · name" pill, joins Saved, one tab per canvas | part | yes | shell-core `serve` | |
| Canvas | Share sheet (links, people search, invite, members, resend / withdraw) | no | yes | W `canvas/share.rs` → shell-core | L: MCP only |
| Canvas | Join links (`copper://` + `https://<cloud>/join/`), pending join kept until sign-in | part | yes | shell-core `join` | L no https intercept, no pending |
| Canvas | Invite pill, Invitations list; mentions inbox (pill, badge, mark read), OS notification | no | yes | W `inbox.rs`, `pills.rs` → shell-core | |
| Canvas | Chat panel, send @mentions / assignment notices | yes | yes | shell-core `host` | |
| Canvas | Checklists, comments, assignees, timeline, comment threads (page) | yes | yes | canvas-app bundle | |
| Canvas | Live frames + posters; frames on sites that forbid embedding | part | part | engine | no XFO bypass |
| Canvas | MCP canvas tools, paged read, screenshot waits for settled, staged ready-wait | yes | yes | shell-core `canvas::mcp` | `canvas_list.invitations` on both |
| Canvas | "Switching from Chrome" canvas after import | yes | yes | shell-core `land_chrome_canvas` | W Move in's "Open the Chrome guide" |
| Canvas | Xos whiteboard insert (`copper://canvas/insert`) | no | no | shell-core `canvas::xos` (to add) | |
| Canvas | Easel → canvas import | mac | mac | — | easels only existed on the Mac |
| Calendar | Calendar peek on the Google Calendar pin (next 4 meetings, Join links, clock) | yes | yes | shell-core `calendar`, copper-calendar | |
| Calendar | Meetings source | part | part | copper-calendar `feeds` | iCal feeds instead of EventKit (mac) |
| Settings | ⌘, panel; ten pages in four groups; blurbs | part | yes | platform | L 9 pages (no Spaces), no groups/blurbs |
| Settings | Reopens at last page; open at a row (deep link) | part | yes | core `SETTINGS_PAGE` | L no deep link |
| Settings | Search (typo-tolerant, grouped), opens folded rows, ⌘F, ↑↓↩, Esc clears | part | yes | W `search.rs` → shell-core | L adw search: titles only |
| Settings | ⌘W closes Settings; typing starts a search | no | no | platform | |
| Settings | Asks before destructive actions | part | part | platform | W addon Remove doesn't ask |
| Settings | General: default browser, appearance, Move in, bring in history, Downloads folder, Ask where | part | yes | core | L no history bring-in, no Ask where |
| Settings | General: Correct spelling as you type | no | part | core `AUTOCORRECT` | engine spellcheck is the stand-in |
| Settings | Tabs: sidebar, show, ⌃Tab order, swipe, sleep, archive, calendar feeds | part | part | core | swipe inert on both |
| Settings | Spaces page: pick, name, look, profile, icon, colours, adjust, delete | part | part | shell-core | no look / adjust on either |
| Settings | Extensions: Chrome extensions manager | mac | no | — | L WebKitGTK has no WebExtensions; W WebView2 can |
| Settings | Agents: ⌘E context + tool rounds; agent apps; other MCP servers; terminal setup | part | part | copper-mcp | W has context + rounds |
| Settings | Passwords: fill codes, fill addresses/cards, offer passkeys, CSV | part | part | shell-core | passkeys only |
| Settings | Privacy: block, block on this site, forget camera/mic, clear history / cookies / cache | yes | part | shield, core | forget choices missing |
| Settings | About: identity, Send Feedback, Diagnostics, shortcuts, Updates | part | part | platform | L check + link only |
| Move in | One sheet: choose → working → summary; keeps running when closed | yes | part | copper-import | |
| Move in | Chrome, Edge, Brave, Chromium, Vivaldi | part | yes | copper-import `browsers` | L no Vivaldi row |
| Move in | Arc sidebar (spaces, pins, folders) | n/a | part | copper-import | W session tabs only; no Arc on Linux |
| Move in | Safari, Dia | mac | mac | — | Mac-only browsers |
| Move in | Bookmarks, history, open tabs → sleeping spaces, profile picker | yes | yes | copper-import | |
| Move in | Passwords from the browser's store | yes | no | copper-import `passwords` | W by design: CSV export only |
| Move in | Export files: passwords CSV, bookmarks HTML | no | part | copper-import | no bookmarks-HTML parser in Rust |
| Move in | Cookies (stay signed in), localStorage | no | no | copper-import (to add) | |
| Move in | Extensions, passkeys | mac | no | — | depend on extensions / passkey store |
| Move in | Counts before moving; second move adds only new; "Work (Chrome)" clash | part | part | copper-import | tabs duplicate on a rerun |
| Move in | Locked-card access explanation | mac | part | — | TCC is macOS; L/W show keyring / in-use errors |
| Extensions | Install from the Chrome Web Store, popups, pages, worker, native messaging, manager | mac | no | new crate (CRX) | W engine enables extensions; CRX + UI missing |
| Signing | Release signing keeps OS grants | mac | part | CI | W optional Authenticode |

## Mac-only, in one line each

- **Menu bar, Dock, traffic lights, scroll bounce, launch watchdog**: AppKit / SwiftUI.
- **Chrome extensions on Linux**: WebKitGTK 2.52 has no WebExtensions API. (Windows is *not* Mac-only: WebView2 supports extensions.)
- **1Password extension sign-in / app link**: needs WebExtensions with native messaging.
- **Phonon-2 speech model**: Core ML on the Neural Engine; another STT is needed elsewhere.
- **Calendar from EventKit**: iCal feeds do the same job off the Mac.
- **Safari and Dia as Move in sources; Easel → canvas import; TCC locked cards; Touch ID gates; signing-tied grants**: macOS-only software or frameworks.

## Keeping this current

- A Mac PR with a user-visible change gets a row here (or an update to one), in the same week.
- Shared logic goes in copper-linux's toolkit-free crates first; Linux and Windows then wire it. A feature only one platform has is a smell — move its logic down.
- Re-audit: `git log --since=<last audit> fork` in the Mac repo, one row per entry that a user would notice, then update the commit at the top.
