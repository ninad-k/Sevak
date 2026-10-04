# Frequently asked questions

## Getting started

### How do I change the hotkey?

Open **Settings → General**, then change **Shortcut** to a key combination not already in use by your desktop or another app. Click **Save**.

On **Linux Wayland**, click **Set up GNOME shortcut** to create the binding in your desktop.

More: [Custom hotkeys](configuration.md#hotkey).

### How do I make Sevak start at login?

Open **Settings → General** and turn on **Launch at login**. Click **Save**.

### Why doesn't my search show any results?

1. Check that the plugin is **enabled** in **Settings → Plugins** (e.g., Applications, Files)
2. If searching files with `f `, make sure the folder is added in **Settings → Files & bookmarks → Folders to search**
3. Reload the index: click the tray icon and select **Reload index**

### Why is a recent app or file missing?

After installing an app or adding files, choose **Reload index** from the tray menu to refresh Sevak's index. New results appear on the next search.

### How do I search inside a folder?

Type the path: `~/Documents/` (note the trailing slash). Sevak lists the contents. Use **↑/↓** to select and **Tab** to drill down. **Shift+Tab** goes back up.

For ordinary searches to also show file results, turn on **Settings → Search → Show in global results**.

## Universal Actions

### What is Universal Actions?

Press ++ctrl+alt+space++ (or your configured hotkey) to show what you can do with what you have selected: text, a link, or files in a file manager.

Sevak copies the selection, shows the available actions and puts your clipboard back. Read more: [Universal Actions](usage.md#universal-actions).

### Why doesn't Universal Actions work on Wayland?

Applications on Wayland cannot read each other's selections or send keystrokes. Workaround: enable **Use the clipboard if the selection can't be read** in **Settings → General**, copy the text yourself, then press ++ctrl+alt+space++.

### Why doesn't Universal Actions work in terminal windows?

On Windows and Linux, Sevak skips terminal windows (because ++ctrl+c++ would interrupt the running program). Select the text with your mouse and use the clipboard fallback instead.

## Appearance and display

### How do I use a dark theme?

Open **Settings → Appearance** and set **Theme** to **Dark**. Click **Save**.

Or set `[appearance] theme = "dark"` in `config.toml`.

### Can I customize the colors?

Yes. Open **Settings → Appearance**:

- **Accent color**: set the highlight color (#rrggbb or rgb(…))
- **Custom stylesheet**: name a CSS file in your config folder to override any theme variable

More: [Themes and appearance](themes.md).

### The launcher is too small or too big. Can I resize it?

Open **Settings → Appearance** and adjust **Window width**. The height scales with the font size.

To change the font size, adjust **Font size** under Appearance (affects the whole launcher).

## Files and searching

### How do I search only files?

Type the keyword followed by a space and filename. Default keyword is `f `: `f project`.

Change or disable the keyword in **Settings → Files & bookmarks → Keyword**.

More: [File search](usage.md#files-and-folders).

### Can Sevak search inside document text?

Yes, through your operating system's own file index: type `in` and the words, for example `in invoice 2026`. `ff report` searches file names anywhere on the disk. `f` still searches names in the folders you chose. The queries go only to the local index (Windows Search, Spotlight, Tracker or Baloo), never over the network. See [Whole-disk and content search](features/files.md#whole-disk-and-content-search).

### The file index is incomplete or seems stuck.

1. Check in **Settings → Files & bookmarks** that the folder is listed
2. Check that **Folder depth** is enough for your nested folders
3. Choose **Reload index** from the tray menu
4. Wait for indexing to finish (check the launcher: it shows a status message if indexing is running)

If a folder is slow or unreachable, indexing continues without blocking.

## Calculations and conversions

### Why doesn't currency conversion work?

Currency conversion is **off by default** because it needs a network connection. Turn it on in **Settings → Plugins** under **Currency conversion**. Click **Save**.

On the first search, Sevak downloads exchange rates from the European Central Bank and caches them for 24 hours.

### Can I use other currencies?

Sevak supports 30+ ISO currency codes and symbols. Use `100 usd in eur`, `50 € to $`, or `$100 in eur`. See [Currency conversion](usage.md#currency-conversion).

### How do I calculate percentages?

The `%` operator is remainder, not percentage. For 15% of 200, type `200*15/100` or `200*0.15`.

More: [Calculator](usage.md#calculator).

## Customization and advanced

### How do I add a keyboard shortcut that runs a command?

In **Settings → Hotkeys**, click **Add hotkey**:

- Set the key (e.g., ++ctrl+alt+t++)
- Choose **Query** and type `> ` to open the launcher with a shell command ready
- Or choose **Run** and type a result ID (e.g., `system:lock`)

More: [Custom hotkeys](configuration.md#hotkey).

### Can I add my own search engine?

Yes. Open **Settings → Web search**, click **Add engine** and fill in:

- **Keyword**: what you type (e.g., `so` for Stack Overflow)
- **Name**: display name
- **URL**: search URL with `{query}` placeholder (e.g., `https://stackoverflow.com/search?q={query}`)

Click **Save**.

More: [Web search keywords](configuration.md#web_search).

### Can I preview a file without opening it?

Yes. Tap ++shift++ or press ++ctrl+y++ to open the preview pane under the results: text files, images, the first page of a PDF, folders, links, snippets and clipboard entries. ++ctrl+t++ opens a long text in a scrollable view. See [Preview, Text View and Grid View](usage.md#preview-text-view-and-grid-view).

### Can Sevak expand text as I type, like a text expander?

Yes, opt-in. Give a snippet a `keyword` and turn on **Settings → Plugins → Expand snippets as you type**. While it is on, Sevak watches your keystrokes (the last 64 characters, in memory only). See [Expand snippets as you type](features/snippets.md#expand-snippets-as-you-type).

### What is the difference between a script plugin and a workflow?

A script plugin adds a keyword whose results come from your script. A [workflow](workflows.md) chains a trigger (a keyword, a hotkey, Universal Actions, `sevak --trigger`) to several steps, such as running a script, transforming text, opening a link, pasting and showing a notification, built visually in **Settings → Workflows**. Both ask for permission before they run code.

### How do I write a script plugin?

Script plugins let you add custom keywords without rebuilding Sevak. Put a folder with a `plugin.toml` and a script in the `plugins` folder next to `config.toml`.

See [Script plugins](usage.md#script-plugins) and the [Plugins guide](plugins.md#external-plugins). Three examples are in `examples/plugins/`.

## Privacy and data

### Does Sevak send my data anywhere?

No. Your searches, files, clipboard and settings stay on your machine. The network requests Sevak itself makes are:

1. **Links you open**: web searches and bookmarks open in your browser
2. **Update checks** (on by default, `[general] check_for_updates`): checks GitHub at startup, every six hours and when you open it; an update is downloaded only after you agree
3. **Currency rates** (off by default): downloads ECB daily rates once per day if you enable it
4. **Theme gallery** and **workflow gallery**: only when you click **Browse online themes** or **Load gallery**, and a package only when you click **Install**

Nothing is uploaded. [Read the full privacy policy.](privacy.md)

### Where are my settings and data stored?

See [Files and data locations](files-and-data.md).

### Can I sync my config to another machine?

Yes. Copy your config folder to another computer (or store it in a synced folder like Dropbox). Sevak reads it on startup.

See [Files and data locations](files-and-data.md) for where your config is stored.

## Troubleshooting

### The launcher won't open. What do I do?

1. Check that Sevak is running: look for it in your system processes
2. Try the CLI: open a terminal and run `sevak` or `sevak --toggle`
3. Restart Sevak: run `sevak --quit`, then open it again
4. Check the logs: **Settings → Open config file** folder, look for logs

More: [Troubleshooting guide](troubleshooting.md).

### I see an error message in Settings. What does it mean?

Validation errors appear under the field that has a problem (shown in red). Fix the value and try again. Common issues:

- **Hotkey already in use**: the key is registered by another app. Choose a different one.
- **Invalid path**: the file or folder does not exist or is not accessible. Check the path.
- **Invalid format**: the value does not match the required format (e.g., color must be `#rgb` or `#rrggbb`)

More: [Troubleshooting](troubleshooting.md).

### Sevak takes a long time to start. Can I speed it up?

1. Reduce the number of indexed folders in **Settings → Files & bookmarks → Folders to search**
2. Lower **Folder depth** to skip deep nested directories
3. Disable plugins you don't use in **Settings → Plugins**
4. If indexing is slow, exclude large generated directories (e.g., `node_modules`, `.git`) — they are usually pruned automatically, but check your index

You can run Sevak in the background with `sevak --background` and press the hotkey to show it when you need it.

### How do I reset Sevak?

Sevak reads `config.toml` at startup. To reset to defaults:

1. Close Sevak: `sevak --quit`
2. Open the config folder (**Settings → Open config file** folder)
3. Delete `config.toml`
4. Restart Sevak; a fresh config is created with defaults

Settings, usage history and clipboard history are in the same folder on Windows and macOS (see [Files and data](files-and-data.md) for Linux).

## Wayland (Linux)

### Why doesn't the shortcut work on GNOME Wayland?

Wayland prevents apps from registering global hotkeys directly. Sevak asks you to run `sevak --setup-hotkey` to create a GNOME desktop shortcut instead.

In Settings, click **Set up GNOME shortcut** to do this automatically, or run the command from a terminal.

More: [Setting up the hotkey on Linux](install.md#setting-up-the-hotkey-on-linux).

### Can I use Sevak on other Wayland desktops (KDE, Weston, etc.)?

Partial support. On KDE and other desktops, you can:

- Use `sevak --toggle` from a terminal or custom keybind
- Leave Sevak running and use the tray icon

Global hotkeys are not possible without desktop-specific setup (if available). Universal Actions and pasting do not work on Wayland.

See [Installing on Linux](install.md).

## macOS

### How do I allow pasting and Universal Actions?

**System Settings → Privacy & Security → Accessibility**: add Sevak to the list. This permission lets Sevak paste results and read your selection for Universal Actions.

### Why doesn't Cmd+Space work?

Cmd+Space is Spotlight's key and Sevak's default. The first time it cannot register it, Sevak asks whether it may turn off Spotlight's shortcut; if you said No, it uses Option+Space instead. You can change your mind in **Settings → General** (**Let Sevak use Cmd+Space**, or **Restore Spotlight's shortcut**). See [Win+Space, Cmd+Space and Super+Space](troubleshooting.md#super-space).

## Performance and limits

### How many files can Sevak index?

The index is capped at **100,000 entries**. If you hit the limit, reduce the number of folders or lower the depth.

### Is there a limit to how many snippets I can create?

No hard limit, but very large configs (100+ entries) may slow down loading slightly.

### Why is search slow when I have thousands of files?

File search uses fuzzy matching on filenames, which is fast. If search is slow, check:

1. Are you searching a very large folder tree? Reduce **Folder depth** or add specific folders only
2. Is indexing still running? Look for a status message in the launcher
3. Are you on a slow or network drive? Local drives are faster

## Questions?

Still stuck? Check the [Troubleshooting guide](troubleshooting.md) or open an issue on [GitHub](https://github.com/ninad-k/Sevak/issues).
