# Discord Rich Presence

Jank! can show **"Playing Jank!"** on your Discord profile with what you are working on:

```
Jank!
Working on "Butterfly demo"          <- or "Working on a project" (name hidden)
Editing polygons · 96 vertices, 72 faces
00:12:31 elapsed
```

A **GitHub** button links to <https://github.com/wwwthresis/Jank/> (Discord shows buttons to
other people only, never on your own card).

The large picture is the Jank! logo, the small round badge shows the mode (Model, Edit, Texture,
Pose, Animate, Render, Export, or a clock after five minutes without input: "Taking a break").

## Privacy

Settings → Discord:

| Switch | Default | Off means |
|---|---|---|
| Show my activity on Discord | on | nothing is sent; the status disappears at once |
| Show the project name | **off** | "Working on a project" |
| Show what I am doing | on | "Making low-poly art", no mode badge |
| Show the elapsed time | on | no timer |

Jank! talks only to the Discord app on the same computer through Discord's local IPC socket
(`discord-ipc-0..9` in `$XDG_RUNTIME_DIR`/`$TMPDIR`/`/tmp`, also the Flatpak and Snap folders;
`\\.\pipe\discord-ipc-N` on Windows). It sends no token, opens no network connection and needs
no SDK. If Discord is not running, Jank! looks for it every 10 seconds; updates are sent at most
every 4 seconds (Discord allows 5 per 20 seconds) and the activity is cleared when Jank! closes.

## Setting up the Discord application (once, by the owner)

1. Open <https://discord.com/developers/applications> → **New Application**, name it **Jank!**
   (this name is what people see after "Playing").
2. Copy the **Application ID** (General Information). It is public; the *Client Secret* and any
   *bot token* are **not** needed and must never be put into the program.
3. **Rich Presence → Art Assets**: upload the nine pictures from `docs/discord/` with their file
   names as keys: `jank_logo`, `mode_model`, `mode_edit`, `mode_texture`, `mode_pose`,
   `mode_animate`, `mode_render`, `mode_export`, `mode_idle`. Discord may take a few minutes to
   show new assets. Regenerate them with `python3 tools/gen_discord_assets.py`.
4. The Application ID of the official build is `1567497552780075190` (`APPLICATION_ID` in
   `core/src/discord.rs`). Forks can use their own: change the constant, or set it when building
   (`JANK_DISCORD_CLIENT_ID=… cargo build --release`) or when starting the program
   (`JANK_DISCORD_CLIENT_ID=… jank`). Builds with an empty ID show "This build has no Discord
   Application ID yet" in Settings and never touch the socket.

The code is `core/src/discord.rs` (protocol, connection thread, the texts; tested against a fake
Discord server) and `app/src/discord_ui.rs` (editor state → activity, the Settings page).
