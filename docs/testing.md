# Testing

How Sevak is tested, what only a person can check, and the checklist to run
before a release. For setting up a development machine see
[Development](development.md).

## The testing pyramid

Most of Sevak is checked automatically on every push. A launcher also depends
on things no test can see: the keyboard shortcut reaching it past the OS, the
permission dialogs of the OS, other people's apps receiving a paste. The
[release checklist](#release-test-checklist) covers that second kind.

| Layer | What it checks | Where | Runs |
|---|---|---|---|
| Static checks | Formatting, clippy with warnings as errors, `svelte-check` type checking, `mkdocs build --strict` for broken links | `cargo fmt`, `cargo clippy`, `npm run check`, docs workflow | Every push (Windows, macOS, Ubuntu) |
| Rust unit tests | Fuzzy matching, config parsing and comment-preserving saves, ranking, every plugin's logic (calculator, units, snippets, clipboard history, tasks, workflows ...), platform helpers | `#[cfg(test)]` modules in `crates/*/src` | Every push, on each OS (platform code runs on its own OS) |
| Rust integration tests | The real search engine and registry with a recording platform: keyword routing, secrets never leaking into global queries, Enter reaching the platform; script plugins and workflows with real child processes | `crates/sevak-plugins/tests/` | Every push |
| UI unit tests | Accelerator parsing and identity, settings validation, path helpers, theme colour and contrast maths, theme JSON, workflow graph rules, a few components | `ui/src/**/*.test.ts` (Vitest) | Every push (`UI unit tests` job) |
| Latency budget | The default engine over a large mock index answers typical queries in under 100 ms at p95 | `crates/sevak-plugins/tests/latency.rs` | Every push (`Search latency budget` job, optimized build) |
| Benchmark | Precise latency numbers and startup index time | `crates/sevak-plugins/benches/` | Weekly and on demand (`Benchmark` workflow) |
| Coverage | Which lines the tests run | `Coverage` workflow | Every pull request (report only) and weekly |
| Build and packaging | The installers build on all three OSes and Fedora builds and tests | `bundle` and `fedora` jobs in `ci.yml` | Every push |
| **Manual release test** | Everything that needs a real desktop, real apps, real permissions | [below](#release-test-checklist) | Before each release |

### What automated tests cannot tell you

These are why the manual plan exists. None of them can be checked from a test
runner, whatever the coverage number says.

- A global hotkey really reaching Sevak instead of the OS or another app
  (the Windows keyboard hook, the Spotlight shortcut, GNOME's input-source
  shortcut, Wayland's lack of global keys).
- Permission dialogs and their wording: macOS Accessibility, Input Monitoring,
  Contacts; SmartScreen and Gatekeeper on first run.
- Pasting into, and reading a selection from, other people's applications.
- The tray or menu-bar icon and its menu on each desktop environment.
- What an installer, an upgrade or an uninstall leaves behind.
- Visual quality: themes, blur, high-DPI, multiple monitors.
- Screen readers and the other accessibility tools.
- Real services: browsers' bookmark files, the OS file index, media players,
  password managers.

## Running the automated tests

Run from the repository root. CI pins Rust 1.99.0 for clippy; `cargo +1.99.0 ...`
reproduces it.

| Command | What it does |
|---|---|
| `cargo fmt --all --check` | Formatting |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints (benches and tests included) |
| `cargo test --workspace` | All Rust tests (the latency test skips itself in a debug build) |
| `cargo test --profile fast-release -p sevak-plugins --test latency -- --nocapture` | The latency budget, with timings (the profile is optimized without link-time optimization, so it builds in a few minutes) |
| `cargo bench -p sevak-plugins` | The latency benchmark (uses the full release profile, so the first build is slow) |
| `npm ci` then `npm run check` | Install and type-check the UI |
| `npm test` | UI unit tests (Vitest, a few seconds) |
| `npm run test:watch` | Vitest in watch mode |
| `npm run test:coverage` | UI tests with a coverage report in `ui/coverage/` |
| `mkdocs build --strict` | Documentation build (needs `pip install -r docs/requirements.txt`) |

Two rules for tests in this repository:

- **No real-time sleeps for correctness.** A test that waits 50 ms and hopes is
  flaky on a busy runner. Wait for a condition with a generous timeout, or hold
  the mock so the order of events is fixed.
- **No network, and nothing outside a temp directory.** Tests use a mock
  platform and `tempfile`; they must not touch the real clipboard, config
  folder or browser profiles.

The unit tests of the pure UI logic live next to the code (`accelerator.test.ts`
beside `accelerator.ts`). A test that needs a DOM starts with
`// @vitest-environment jsdom`; Svelte components are tested with
`@testing-library/svelte`, which is worth doing only for small, self-contained
components (`Toggle.test.ts` is the example).

### Coverage

Coverage is reported, not enforced: there is no threshold. The `Coverage`
workflow runs on pull requests and weekly, writes the tables below into the job
summary and attaches the lcov and HTML reports as an artifact named `coverage`.
No outside service is involved.

To reproduce the Rust numbers locally:

```sh
rustup component add llvm-tools-preview
cargo install --locked cargo-llvm-cov
cargo llvm-cov --no-report -p sevak-core -p sevak-platform -p sevak-plugins \
  --ignore-filename-regex '([/\\](tests|benches)[/\\]|test_util\.rs)'
cargo llvm-cov report --html     # target/llvm-cov/html/index.html
cargo llvm-cov report --json --summary-only --output-path rust-summary.json
node scripts/coverage-summary.mjs --rust rust-summary.json --ui ui/coverage/coverage-summary.json
```

**Reference numbers** (Windows host, so the Windows backend of `sevak-platform` is
what runs; the Linux numbers come from the `Coverage` workflow and differ for that
crate). Measured with `cargo llvm-cov` and Vitest.

| Crate | Lines | Functions | Regions |
|---|---|---|---|
| sevak-core | 97.1% (5585/5749) | 95.0% (683/719) | 97.0% (10117/10433) |
| sevak-platform (Windows backend) | 79.4% (9819/12369) | 76.9% (1299/1690) | 80.7% (16791/20796) |
| sevak-plugins | 95.6% (25626/26818) | 92.8% (2847/3069) | 95.6% (45304/47378) |
| **total** | 91.3% (41030/44936) | 88.2% (4829/5478) | 91.9% (72212/78607) |

These Rust numbers are generous: the unit tests live in `#[cfg(test)]` modules inside
the source files, and llvm-cov counts those lines as code that ran. Read them as an
upper bound and look at the HTML report for the uncovered functions, which is where the
real gaps are (OS calls such as the Windows keyboard hook, and anything that needs a
display). The Tauri shell (`src-tauri`) is not measured: it needs a desktop and the
GTK libraries to build, and its logic is thin glue.

The UI percentages for the whole `ui/src` tree are low (15% of lines) because Svelte
components are not unit-tested; the logic modules that hold the rules are covered:

| File | Lines | Branches |
|---|---|---|
| `lib/accelerator.ts` | 100% | 98% |
| `lib/validate.ts` | 100% | 100% |
| `lib/path.ts` | 100% | 100% |
| `lib/themes.ts` | 100% | 98% |
| `lib/theme.ts`, `lib/appearance.ts` | 100% | 100% |
| `lib/workflows/model.ts` | 83% | 80% |
| all of `ui/src` | 15% | 16% |

### Latency

The engine logs a warning for any query slower than 16 ms
(`sevak_core::engine::LATENCY_BUDGET`), the time that still feels instant. Two
things watch it:

- **`engine_query_latency_budget`** (`crates/sevak-plugins/tests/latency.rs`)
  runs 13 typical queries 40 times each after a warm-up and fails if any
  query's 95th percentile is 100 ms or more. The bound is deliberately six
  times the target: it is there to catch an accidental O(n&sup2;) or a disk read
  on the typing path, not to police a few milliseconds on a shared CI runner. A
  query gets three rounds and passes with the first round in budget, so one
  stalled round does not fail the build; a real regression is slow in every
  round. It needs an optimized build and skips itself under `debug_assertions`.
- **`cargo bench -p sevak-plugins`** measures the same queries with criterion
  (mean, median and confidence interval) plus the startup index build. The
  `Benchmark` workflow runs it weekly on Linux and prints the table in the job
  summary; it never fails on a number.

Both build the same fixture (`crates/sevak-plugins/benches/fixture/mod.rs`):
the real default plugins through the real registry over a mock platform, with
5,000 applications, 100,000 files (the files plugin's own cap), 10,000 bookmarks,
a 200-entry clipboard history and the two example workflows. Nothing reads the
real clipboard, browsers or OS file index. The queries:

| Name | Input | What it exercises |
|---|---|---|
| single letter | `a` | Many plugins, nothing worth scoring |
| short prefix | `fir` | Fuzzy match over apps, files, bookmarks |
| app name | `visual studio` | Two-word match against 5,000 apps |
| multi-word | `budget report meeting` | Three words over 100,000 file names |
| no match anywhere | `qzxvkj` | Worst case: every index scanned, nothing found |
| keyword: web / bookmarks / clipboard / files / workflow | `g rust lifetimes`, `b budget`, `cb copied`, `f invoice`, `ddg rust` | Keyword routing to one plugin |
| path browse | `~/Documents/rep` | A real directory listing of 300 entries |
| calculator | `(12 + 8) * 3 / 4` | The calculator, plus every other global plugin |
| unit conversion | `12 km in miles` | Units, plus every other global plugin |

**Reference numbers.** Mean time per `SearchEngine::query` from `cargo bench`
(release profile, Rust 1.99.0) on the maintainer's development machine: Windows 11,
Intel Core Ultra 9 275HX (24 cores), measured while other builds were running, so
expect the same order of magnitude rather than the same digits. Linux CI runners are
slower and noisier; the weekly `Benchmark` workflow publishes theirs.

| Query | Mean | Median |
|---|---|---|
| single letter (`a`) | 1.06 ms | 1.01 ms |
| short prefix (`fir`) | 21.3 ms | 20.4 ms |
| app name (`visual studio`) | 22.6 ms | 22.0 ms |
| multi-word (`budget report meeting`) | 19.2 ms | 18.9 ms |
| no match anywhere (`qzxvkj`) | 16.1 ms | 16.0 ms |
| keyword: web (`g ...`) | 0.003 ms | 0.003 ms |
| keyword: bookmarks (`b ...`) | 0.91 ms | 0.70 ms |
| keyword: clipboard (`cb ...`) | 0.28 ms | 0.25 ms |
| keyword: files (`f ...`) | 11.7 ms | 10.9 ms |
| keyword: workflow (`ddg ...`) | 0.004 ms | 0.004 ms |
| path browse (`~/Documents/rep`) | 6.8 ms | 6.6 ms |
| calculator | 22.0 ms | 21.0 ms |
| unit conversion | 11.7 ms | 10.5 ms |
| startup: reload of every plugin (5,000 apps, 20,000 files walked on disk, 10,000 bookmarks, 200 clipboard entries) | 103 ms (first run 136 ms) | 100 ms |

What the numbers say: everything that is routed to one plugin by a keyword is far
under the 16 ms line, but a **global query over a 100,000-file index costs 16 to 23 ms**
on this machine, because the files plugin scores every file name on every keystroke.
The files plugin caps its index at 100,000 entries, so this is the worst case; most
home folders hold far fewer. It is the number to watch if the files plugin changes. The startup row is a worst case for the walk: the 100,000-entry index
used by the query rows is injected rather than walked, because creating that many files
on every run would measure the disk.

## Release test checklist

### How to run the plan

**Who.** The maintainer, plus anyone with a machine from the matrix below. One
person can cover several rows with VMs, but Wayland, Apple silicon and a
physical Windows machine are worth real hardware.

**When.** Before every release that changes behaviour (any `feat:` or `fix:`
commit since the last tag), on the release build, not a development build: use
the installers from the draft release or the `bundle` artifacts of the commit.
A docs-only release needs nothing. A release that changed one area can run only
that area plus the **smoke test** (rows marked **S**) on every OS.

**How.**

1. Open a new issue from the
   [Release test report](https://github.com/ninad-k/Sevak/issues/new?template=release_test_report.yml)
   template, one per OS and version. It asks for the build, the machine and
   the IDs of failing rows.
2. Use a fresh user account or VM snapshot for the install rows, and a normal
   one for the rest. Back up `config.toml` first; the plan edits it.
3. Work through the tables for that OS. Record only the failures and the
   rows you could not run, with the ID (for example `CLP-4`), what happened and
   a link to the bug you filed.
4. Rate each failure with the severity below. Any **S1** blocks the release; an
   **S2** needs a fix or an explicit waiver from the maintainer in the report.

**Safety.** Never test destructive rows on a machine you care about. Use a
scratch folder for file operations, and a throwaway process for `kill`. Restart,
shut down, log out and empty trash are checked only as far as their
confirmation prompt, then cancelled.

### Severity

| Level | Meaning | Example |
|---|---|---|
| **S1** | Blocks the release: data loss, a crash or hang, the launcher unreachable, a privacy or security failure | Hotkey never fires; a copied password lands in the history; an update installs unverified |
| **S2** | A feature is broken on a supported platform | Bookmarks of one browser never load; paste does nothing on macOS |
| **S3** | Works but wrong in a narrow case, or a clear usability flaw | A tooltip is cut off; a rare layout is misaligned |
| **S4** | Cosmetic | A pixel off |

### The test machines

| Code | System | Notes |
|---|---|---|
| W10 | Windows 10 22H2 | Also check a machine with a non-US keyboard layout |
| W11 | Windows 11 | Primary Windows target |
| MI | macOS on an Intel Mac (11+) | Universal build |
| MA | macOS on Apple silicon | Universal build; Rosetta not needed |
| UX | Ubuntu 22.04 or 24.04 on GNOME, X11 session | `.deb` |
| UW | Ubuntu on GNOME, Wayland session | `.deb`, `sevak --setup-hotkey` |
| FW | Fedora (current) on GNOME, Wayland session | `.rpm`; AppIndicator extension needed for the tray |

Optional extras worth a smoke test when available: KDE Plasma (X11 and
Wayland), a second monitor with a different scale factor, Windows with
high-contrast on, and an account without administrator rights.

In the tables, **OS** lists where a row applies (`all` is every machine
above; `W` is both Windows versions, `M` both Macs, `L` the three Linux
setups). **Auto** says what the automated tests already cover, so a tester
knows what is checked by hand because it cannot be checked by a machine.
**Sev** is the severity if the row fails. Rows marked **S** are the smoke test.

### Install, upgrade, uninstall

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| INS-1 **S** | W | Download the NSIS `-setup.exe` from the release. Run it as a standard user | SmartScreen warning is the only obstacle ("More info", "Run anyway"); installs without administrator rights; Start menu entry; Sevak starts | Bundle job builds it; install itself is manual | S1 |
| INS-2 | W | Install the `.msi` as administrator | Installs per machine; Start menu entry for all users; Sevak starts | Bundle job builds it | S2 |
| INS-3 **S** | M | Open the `.dmg`, drag Sevak to Applications, open it. Use "Open Anyway" in Privacy & Security | One-time Gatekeeper prompt; menu-bar icon (no Dock icon); the launcher opens | Bundle job builds it | S1 |
| INS-4 **S** | L | `sudo apt install ./Sevak_*.deb` (Ubuntu) or `sudo dnf install ./Sevak-*.rpm` (Fedora). Start from the application menu | Installs with its dependencies; entry in the menu with a "Show or hide" action; Sevak starts | Bundle and Fedora jobs build and test; install is manual | S1 |
| INS-5 | L | Make the AppImage executable and run it (with FUSE 2, then with `--appimage-extract-and-run`) | Starts both ways | Bundle job builds it | S2 |
| INS-6 **S** | all | Launch Sevak for the first time with no config folder | `config.toml` is created with comments; launcher opens with the default shortcut; no error dialog | Config defaults are unit-tested | S1 |
| INS-7 | all | Install the previous release, change settings, use clipboard history and snippets, then install this release over it | Settings, usage ranking, clipboard history and snippets survive; the launcher works at once | Config round-trips are unit-tested; upgrades are manual | S1 |
| INS-8 | all | Uninstall (Settings > Apps on Windows; delete the app on macOS; `apt remove` or `dnf remove`) | Sevak disappears from the menus and stops; the config and data folders are left in place (see [Files and data](files-and-data.md)); the launch-at-login entry is removed (check Startup apps or Login Items) | None | S2 |
| INS-9 | all | Uninstall, then reinstall | Old settings are picked up | None | S3 |

#### Installer scenarios

!!! note "Pending the branded installer"
    A separate effort is building the branded Windows installer. Until it lands,
    run these against the NSIS and MSI packages above and note the deviations.
    Update this section when the installer ships.

| ID | OS | Scenario | Steps | Expected | Sev |
|---|---|---|---|---|---|
| INS-A1 | W | Fresh, per user | Run the installer as a standard user on a clean account | No administrator prompt; installed under the user's profile; Start menu and uninstall entry for that user only | S1 |
| INS-A2 | W | Fresh, per machine | Run the machine-wide installer as administrator | Installed for all users; another account sees the Start menu entry | S2 |
| INS-A3 | W | Upgrade | Install release N, run it, then install N+1 over it without uninstalling | Sevak (running or not) is replaced; no duplicate Start menu entries; no second uninstall entry; settings kept | S1 |
| INS-A4 | W | Downgrade | Install N+1, then run the installer of N | Either refuses with a clear message or installs N cleanly; config written by N+1 still loads or is reported, never silently reset | S2 |
| INS-A5 | W | Per-user to per-machine | Install per user, then install per machine | The second install detects the first (or the two do not clash); one working Sevak remains | S2 |
| INS-A6 | W | Silent install and uninstall | Run the installer with its silent switch from a script; then the silent uninstall | No UI; exit code 0; installed and removed correctly | S2 |
| INS-A7 | W | Running while upgrading | Leave Sevak running (tray icon) and upgrade | The installer closes it or asks; the new version starts afterwards; the Win+Space hook is released while it is closed | S2 |
| INS-A8 | W | Repair | Delete the installed executable, then repair or reinstall | Restores a working install | S3 |

### Launch and hotkey

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| LCH-1 **S** | all | Press the shortcut shown in Settings > General | The launcher appears centred on the screen with the mouse, with the search box focused | Window logic is manual | S1 |
| LCH-2 **S** | all | With the launcher open: press Esc; reopen and click another window | Esc hides it; losing focus hides it (when "Hide when focus is lost" is on) | None | S2 |
| LCH-3 | all | Start Sevak a second time from the menu or `sevak` | No second instance or icon; the running one shows the launcher | Single-instance plugin | S2 |
| LCH-4 | all | Set `[general] hotkey` to something else in Settings and save; try the old and new keys | New key works at once, old key stops; conflicts show an inline message and the old key keeps working | Accelerator parsing and duplicate checks are unit-tested | S2 |
| LCH-5 | all | Add a custom hotkey (`query = "> "` and `run = "..."`) and the Universal Actions key; press each | The launcher opens with the text / the result runs / Universal Actions opens | Hotkey config parsing is unit-tested | S2 |
| LCH-6 | all | Turn "Launch at login" on, sign out and in; then off | Starts hidden after login; no start after turning off | None | S2 |
| LCH-7 | all | Multiple monitors: move the mouse to the second screen (different scale) and open the launcher | Appears on that screen at a readable size | None | S3 |

#### Windows

| ID | OS | Steps | Expected | Sev |
|---|---|---|---|---|
| LCH-W1 **S** | W | With the default Win+Space, read the status line in Settings > General | "Taken over with the Windows keyboard hook" | S1 |
| LCH-W2 **S** | W | Press Win+Space in Notepad | Only the launcher opens: the language switcher flyout does not appear and the Start menu does not open when Win is released | S1 |
| LCH-W3 | W | Press Win+Shift+Space with two input languages installed | Windows' previous-language shortcut still works | S2 |
| LCH-W4 | W | Quit Sevak, press Win+Space | Windows switches language again (the key is released) | S1 |
| LCH-W5 | W | Focus an elevated (administrator) window and press the shortcut | Documented limit: nothing happens; running Sevak as administrator makes it work | S3 |
| LCH-W6 | W | Choose Alt+Space in Settings | Registers normally (status line empty) | S2 |

#### macOS

| ID | OS | Steps | Expected | Sev |
|---|---|---|---|---|
| LCH-M1 **S** | M | First launch with the default Cmd+Space while Spotlight's shortcut is on | One dialog: "Cmd+Space is used by Spotlight. Let Sevak use it?" | S1 |
| LCH-M2 **S** | M | Answer Yes, then press Cmd+Space (log out and in if needed) | Sevak opens, Spotlight does not; the status line says Spotlight's shortcut is off with your permission | S1 |
| LCH-M3 | M | Answer No (reset with the restore button first) | Option+Space is used and the question is not asked again | S2 |
| LCH-M4 | M | Press the Restore Spotlight button in Settings | Spotlight's shortcut returns; Sevak moves off Cmd+Space | S2 |
| LCH-M5 | M | On the first run check Dock and Cmd+Tab | No Dock icon, Sevak is not in the app switcher; menu-bar icon present | S3 |

#### Linux

| ID | OS | Steps | Expected | Sev |
|---|---|---|---|---|
| LCH-L1 **S** | UX | Start Sevak on GNOME X11 with the default Super+Space | One prompt offers to move GNOME's input-source shortcut to Ctrl+Super+Space; after Yes the launcher opens with Super+Space | S1 |
| LCH-L2 | UX | Press Restore in Settings or run `sevak --restore-hotkey` | GNOME's shortcut is back to its old value | S2 |
| LCH-L3 | UX | Choose Alt+Space | Warns about GNOME's window-menu conflict, or fails with a readable message; Ctrl+Space works | S3 |
| LCH-L4 **S** | UW, FW | Run `sevak --setup-hotkey` (or the Settings button) | Creates GNOME custom shortcuts for toggle, Universal Actions and each `[[hotkey]]`; pressing the key shows the launcher | S1 |
| LCH-L5 | UW, FW | Edit a `[[hotkey]]`, run `--setup-hotkey` again | Shortcut updated, no duplicates | S3 |
| LCH-L6 | UW, FW | Open the launcher: check position and typing | Opens (via XWayland by default) centred and accepts input | S2 |
| LCH-L7 | L | KDE or Sway: bind a key to `sevak --toggle` | Toggles the launcher | S3 |

### Tray

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| TRY-1 **S** | all | Left-click the tray or menu-bar icon | Settings opens directly | None | S2 |
| TRY-2 **S** | all | Right-click (or on macOS click) the icon | Menu: Show, Settings, Reload index, Check for updates, Quit | None | S2 |
| TRY-3 | all | Choose each item | Show opens the launcher; Settings opens Settings; Reload index re-scans (a newly created file or installed app becomes searchable); Check for updates reports a result; Quit exits and removes the icon | None | S2 |
| TRY-4 | FW | Without the AppIndicator extension | No icon, but Sevak still runs and the hotkey works; with the extension enabled the icon appears | None | S3 |
| TRY-5 | L | `sevak --settings`, `sevak --toggle`, `sevak --quit` from a terminal | Each acts on the running instance | CLI parsing is unit-tested | S2 |

### Search and ranking

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| SRC-1 **S** | all | Type the first letters of an installed app | The app is the first result; Enter launches it and hides the launcher | Matching and ranking: engine and apps tests | S1 |
| SRC-2 | all | Open the same app several times via a different query, then type a short ambiguous prefix | The app you use climbs the list | Usage boost is unit-tested | S3 |
| SRC-3 | all | Type nothing; type only spaces | No results, no error | Engine test | S3 |
| SRC-4 **S** | all | Type `g rust lifetimes`, Enter | Your browser opens a Google search; likewise `yt` and `gh` | Keyword routing and URL building | S2 |
| SRC-5 | all | Type a query matching nothing | The fallback web search offers to search it | Engine fallback test | S3 |
| SRC-6 | all | Arrow keys, Ctrl+N/P, PageUp/PageDown, Ctrl+1 to 9, Tab completion, Ctrl+K action panel | Each key does what [Keyboard shortcuts](keyboard.md) says | None | S2 |
| SRC-7 | all | Shift tap or Ctrl+Y on a file or app result; Ctrl+L; Ctrl+T on a long result | Preview pane, Large Type and Text View appear and dismiss | Preview content is unit-tested | S3 |
| SRC-8 | all | Disable a plugin in Settings > Plugins, save | Its results stop appearing, without restart | Registry disable logic | S3 |
| SRC-9 | all | While a large file index is loading, type quickly | Never freezes; results appear as the index fills | Latency budget (mock index) | S2 |
| SRC-10 | all | Type 20 characters rapidly | No dropped characters or visible lag | Latency budget | S2 |

### Calculator, units and currency

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| CAL-1 **S** | all | `(12 + 8) * 3 / 4`; `2^10`; `15% of 80`; Enter | 15, 1024, 12; Enter copies the result and hides the launcher | Calculator tests | S2 |
| CAL-2 | all | `12 km in miles`; `100 f in c`; `3 gb to mb` | Correct conversions with sensible rounding | Units tests | S2 |
| CAL-3 | all | Currency conversion off (default): `100 usd in eur` | No result and no network request | Opt-in logic tested | S1 |
| CAL-4 | all | Turn on "Currency conversion" in Settings; `100 usd in eur` | Result from the ECB daily rates; with the network off a clear "rates unavailable" row, not a hang | Parsing tested with fixtures; the fetch is manual | S3 |
| CAL-5 | all | A locale with a comma as decimal separator | Input like `1,5 + 2` is handled as documented | Partly tested | S3 |

### Files and the OS index

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| FIL-1 **S** | all | Create a file in Documents named `sevaktest-report.txt`; Reload index; type `sevaktest` | The file is found; Enter opens it; Ctrl+Enter shows it in the file manager; Shift+Enter copies the path | Index and actions are unit-tested | S2 |
| FIL-2 | all | Type `~/Documents/`, then a few letters; Tab; Shift+Tab | Lists the directory; Tab opens the highlighted folder; Shift+Tab goes up | Path browsing is unit-tested (Rust and `path.test.ts`) | S2 |
| FIL-3 | W, M | `ff sevaktest`, then `in <a word inside a text file>` | Name and content search through Windows Search or Spotlight; when the index is unavailable a readable message | OS search parsing tested; the live index is manual | S3 |
| FIL-4 | L | `ff sevaktest` with `locate`, Tracker or Baloo installed, and without | Results from the available tool; a readable message when none | Parsing tested | S3 |
| FIL-5 | M | First file search | macOS asks for access to Desktop, Documents and Downloads; denying it degrades gracefully | None | S3 |
| FIL-6 | all | Hidden files with `include_hidden` on and off | Hidden entries appear only when enabled or when the typed segment starts with a dot | Unit-tested | S3 |

### Bookmarks

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| BKM-1 **S** | all | Bookmark a page with a unique title in your main browser; Reload index; `b <title>` | The bookmark is found; Enter opens it in the default browser | Chromium and Firefox parsers: unit tests | S2 |
| BKM-2 | all | Repeat for each installed browser: Chrome, Edge, Brave, Vivaldi, Opera, Chromium, Firefox (and LibreWolf or Zen) | Found, with the folder path shown | Parsers with fixtures | S2 |
| BKM-3 | all | Firefox running (its database is locked) | Still searchable (a private copy is read) | Copy logic tested | S2 |
| BKM-4 | all | A second browser profile | Its bookmarks appear too | Unit-tested | S3 |
| BKM-5 | all | Limit `[bookmarks] browsers` to one browser | Only that browser's bookmarks | Unit-tested | S3 |

### Clipboard history

Clipboard history is opt-in. Turn it on under Settings > Plugins or with
`[clipboard] enabled = true`, then use Reload index.

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| CLP-1 **S** | all | Copy text in another app; type `cb` | The copied text is the top row; what was on the clipboard before turning the feature on is not there | History logic: about 60 unit tests | S1 |
| CLP-2 | all | Copy an image (screenshot or from a browser); `cb`, then `cb image` | A thumbnail row and a grid view; Enter pastes the image where supported | Image storage tested; capture is manual | S2 |
| CLP-3 | all | Copy files in the file manager | Recorded as a file list; pasting restores them | Tested with mock | S2 |
| CLP-4 **S** | W, M | Copy a password from a password manager that marks it secret (1Password, KeePassXC, Bitwarden desktop) | Never appears in `cb`; the file `clipboard-history.json` does not contain it | The sensitive flag path is unit-tested; real managers are manual | S1 |
| CLP-5 | L | Add the password manager to `ignore_apps`; copy from it | Not recorded (Linux has no secret marker) | `ignore_apps` tested | S1 |
| CLP-6 | all | Copy text longer than `max_item_bytes` (64 KiB) | Not recorded | Tested | S3 |
| CLP-7 | all | Pick a text entry: Enter pastes into Notepad or TextEdit, Ctrl+C copies without pasting; then type `cb clear` and press Enter on the Clear clipboard history row | As labelled; clearing (no further prompt) empties the history and the image files | Tested | S2 |
| CLP-8 | all | Copy 250 distinct texts | The list stops at `max_items` (200); images over the 500 MB total prune oldest first | Trim logic tested | S3 |
| CLP-9 | L, M | Inspect the permissions of `clipboard-history.json` | Readable only by the user (0600) | Unit-tested | S2 |
| CLP-10 | all | Turn the feature off | Nothing more is recorded; no clipboard polling continues | None | S2 |

### Paste into other applications

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| PST-1 **S** | W | From `cb`, `s` (snippets) and the emoji picker, paste into Notepad, a browser text box, Word and a terminal (Windows Terminal) | Text appears at the caret; the launcher hides first | Mock platform; the real keystroke is manual | S1 |
| PST-2 **S** | M | Same, in TextEdit, Safari and Terminal | Pastes with Accessibility granted; without it Sevak copies and says so | None | S1 |
| PST-3 | UX | Same, in gedit and Firefox | Pastes through XTest | None | S1 |
| PST-4 | UW, FW | Same | Copies to the clipboard only, and says to paste with Ctrl+V (Wayland forbids synthetic keys) | Support detection tested | S2 |
| PST-5 | all | With "restore clipboard" on, paste a snippet | The clipboard you had before is back afterwards | Unit-tested | S3 |

### Universal Actions

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| UAC-1 **S** | W, M, UX | Select text in a browser, press Ctrl+Alt+Space | A panel of actions for the selected text (search, transform, copy, ...); Esc hides it | Action building and routing: unit and integration tests | S1 |
| UAC-2 | W, M, UX | Transform text (for example the Tidy whitespace workflow) | The selection in the source app is replaced, or copied if pasting is unavailable | Workflow tested end to end | S2 |
| UAC-3 | all | Select a URL; select a file in the file manager | URL and file actions appear (open, copy path, ...) | Classification tested | S2 |
| UAC-4 | all | Run a Universal Action, then open `cb` | The temporary copy Sevak made to read the selection is not in the history; your previous clipboard is restored | Tested | S1 |
| UAC-5 | UW, FW | Select text and press the key | Uses the clipboard fallback if enabled, else says it cannot read the selection | Tested | S3 |
| UAC-6 | M | Without Accessibility | Explains how to grant it | None | S3 |

### Snippets and auto-expansion

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| SNP-1 **S** | all | `s <name>` with a snippet containing `{date}` and `{clipboard}` | Placeholders are filled; Enter pastes | Placeholder engine tested | S2 |
| SNP-2 | W, M, UX | Turn on "Expand snippets as you type"; add `;sig` in Notepad/TextEdit/gedit | Typing `;sig` is replaced by the text | Matching state machine tested | S2 |
| SNP-3 | W, M, UX | Type the keyword in a browser text field and in a rich editor | Expands | None | S2 |
| SNP-4 | W, M, UX | Type the keyword in a terminal | Not expanded (terminals are skipped unless enabled) | App-skipping tested | S2 |
| SNP-5 | W, M | Focus a native password box and type the keyword | Not expanded and not recorded | Detection is manual | S1 |
| SNP-6 | all | Type the keyword in a web page password field | Documented limit: not detectable; adding the browser to `ignore_apps` stops it | None | S3 |
| SNP-7 | all | Type the keyword and one more character immediately | The keyword is left alone, never the wrong text deleted | Tested | S2 |
| SNP-8 | M | Turn it on for the first time | macOS asks for Input Monitoring; works after allowing and restarting | None | S2 |
| SNP-9 | UW, FW | Open Settings > Plugins | Says expansion is not possible on Wayland | None | S3 |
| SNP-10 | all | Turn the setting off | The keyboard hook / listener is released (no listener running) | None | S1 |

### File buffer

Use a scratch folder with a few throwaway files.

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| FBF-1 **S** | all | On file results press Alt+Up three times; Alt+Left; Alt+Backspace | Chips appear and disappear as described in [Files](features/files.md#file-buffer) | Buffer logic tested | S2 |
| FBF-2 | all | Copy to... a second scratch folder, including a file whose name exists there | Copies; the clash becomes `name (2)`; nothing is overwritten | Tested | S1 |
| FBF-3 | all | Move to... another folder | Moved items leave the buffer; failures stay with a reason | Tested | S2 |
| FBF-4 | all | Compress to .zip | `Archive.zip` appears next to the files and opens in the OS | Zip writing tested | S3 |
| FBF-5 | all | Move to Trash (confirm) on the scratch files | Asks first, naming the items; files are in the Recycle Bin or Trash | Platform call is mocked | S1 |
| FBF-6 | all | Open all with more than 10 items | Asks first | Tested | S3 |
| FBF-7 | all | Try to move a folder into itself | Refused with a message | Tested | S1 |

### Automation tasks, system commands and media

Only non-destructive tasks are run. For anything that restarts, shuts down or
quits something, check that a confirmation appears and cancel it.

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| TSK-1 **S** | all | `t` | The list of tasks the OS supports (a task whose helper is missing is not listed) | Task catalog tested per OS | S3 |
| TSK-2 | all | `t`, then `vol 30`, `vol mute`, `vol unmute` | Volume is set; mute and unmute work. Restore your volume | Parsing tested | S3 |
| TSK-3 | all | `t dark` twice | Toggles dark mode and back | None | S3 |
| TSK-4 | all | `awake 1`, then run the Stop keeping awake task | The machine stays awake for a minute; stops on request | None | S3 |
| TSK-5 | all | Start a throwaway process (for example a second Notepad or `sleep 600`); `kill <name>`; confirm | Asks first; ends only that process | Process listing mocked | S1 |
| TSK-6 | all | `restart`, `shutdown`, `logout`, `empty trash` | Each asks for confirmation; **cancel** every one | Confirmation flow tested | S1 |
| TSK-7 | M | First dark-mode, volume or quit task | macOS asks to control System Events; denying gives a readable message | None | S3 |
| MED-1 | all | Play audio in a browser tab, Spotify or VLC; `play`, `pause`, `next` | The player responds; a now-playing row shows title and app | Command construction tested | S3 |
| MED-2 | L | Without `playerctl` | Buttons are not offered | Tested | S3 |

### Contacts and 1Password (opt-in)

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| CON-1 **S** | all | Fresh install, type `c ada` and `1p` | Nothing leaks into ordinary searches; the plugins say they are off | Integration test: "contacts never answer global queries" | S1 |
| CON-2 | all | Enable contacts with a `.vcf` file | `c name`, `@name` find people; Enter copies the email; secondary actions write an email and call | Integration tests | S3 |
| CON-3 | M | Enable contacts and use the Allow row | macOS asks for Contacts access only then (not at startup); after allowing, contacts are listed | None | S2 |
| CON-4 | L | Evolution address books present | Listed | Parsing tested | S3 |
| CON-5 | all | Enable 1Password with the `op` CLI signed in | `1p <title>` lists titles and websites; no passwords are ever shown or copied; Enter opens the item | Mocked CLI output tested | S1 |
| CON-6 | all | Enable 1Password without `op` | A readable message with what to install | Tested | S3 |

### Script plugins and workflows

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| SCR-1 **S** | all | Copy an example from `examples/plugins/` into the plugins folder; Reload index | A dialog asks to allow it (naming the command); nothing runs before Allow | Approval flow: script and workflow integration tests | S1 |
| SCR-2 | all | Allow it, run its keyword | Results from the script; Enter runs the action | Real child process in integration tests | S2 |
| SCR-3 | all | Change the plugin's `command` in `plugin.toml`, then Reload index | The approval is asked for again | Approval key tested | S1 |
| SCR-4 | all | Choose Not now | The plugin stays off and is asked again the next time Sevak starts | Tested | S3 |
| SCR-5 | all | A broken `plugin.toml` | The plugin is skipped with the reason in the log; Sevak and other plugins unaffected | Tested | S2 |
| WFL-1 **S** | all | Settings > Workflows: create from a template, edit, save, run it by keyword | Saves, validates, runs; the ddg example opens DuckDuckGo | Graph validation and runtime: integration tests, `model.test.ts` | S2 |
| WFL-2 | all | A workflow with a script node | Needs approval; editing it asks again | Tested | S1 |
| WFL-3 | W, M, UX | A hotkey trigger and a Universal Actions trigger | Each starts the workflow | Hotkey binding computation tested | S3 |
| WFL-4 | all | Gallery: open the page (watch the network), press Load gallery, install an entry | Nothing is requested until Load gallery; then only the index file; the package installs after its SHA-256 matches | Checksum, safe unpacking and zip-bomb refusal: unit tests | S1 |
| WFL-5 | all | Install a gallery entry from a staging index whose hash is wrong | Refused with a checksum error; nothing installed | Unit-tested | S1 |

### Themes, appearance and blur

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| THM-1 **S** | all | Settings > Appearance: System, Light, Dark; change the OS theme while Sevak runs | The launcher and Settings follow the OS live for System | `theme.ts` logic tested | S2 |
| THM-2 | all | Theme editor: pick a built-in, change colours, save, apply; export and import it | Preview matches the launcher; contrast warnings for poor combinations; file written to `themes/` | Colour maths, contrast and theme JSON: `themes.test.ts`, Rust theme tests | S3 |
| THM-3 | all | Theme gallery: Load gallery, install a theme | Only the index is requested until you click; the install verifies the SHA-256 | Checksum unit-tested | S1 |
| THM-4 | W, M | Turn on frosted-glass blur | Blur behind the bar; with blur on, text stays readable | None | S3 |
| THM-5 | L | Open the same setting | Blur is not offered, or has no effect without errors | None | S3 |
| THM-6 | all | Accent colour, font family and size, opacity, corner radius, window width; custom CSS | Each applies live and is stored; an invalid value shows an inline error | Validation tested | S3 |

### Settings and the configuration file

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| SET-1 **S** | all | Add `# my comment` lines and an unknown key to `config.toml`; change a setting in Settings, save; reopen the file | Comments and the unknown key are still there; only the changed value differs | Comment-preserving saves: several unit tests | S1 |
| SET-2 | all | Enter an invalid value (duplicate web keyword, bad URL, missing `{query}`, clashing keyword) | Inline error; Save blocked; the keyword rules match what Rust reports | `validate.test.ts` and Rust `settings::validate` tests | S2 |
| SET-3 | all | Break `config.toml` by hand (an unmatched quote) and start Sevak | Starts with default settings and writes the reason to the log; your broken file is still on disk to fix | Parse errors are unit-tested | S1 |
| SET-4 | all | Edit `config.toml` while Sevak runs, then Reload index | Changes take effect | None | S3 |
| SET-5 | all | Open the config file, logs folder and themes folder from Settings | Each opens in the OS file manager or editor | None | S3 |

### Updates

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| UPD-1 **S** | all | Tray > Check for updates on the latest release | "Up to date" (or the update offered); nothing installs without your agreement | Manifest and version logic: `release-version.mjs`, `updater-manifest.mjs`, `updater.rs` tests | S2 |
| UPD-2 | all | Install an older release, check for updates, accept | Downloads, verifies the signature, installs and restarts into the new version; settings kept | Signature check is the updater's; end to end is manual | S1 |
| UPD-3 | all | Serve a manifest with a wrong signature (local test server) | The update is refused and nothing is installed | Manual only | S1 |
| UPD-4 | all | Turn "Check for updates" off in Settings; watch the network | No update request at startup, every six hours, or on opening the launcher | None | S1 |
| UPD-5 | W | Scoop or winget install (when published) | Scoop installs leave updates to the package manager (the tray names the command); winget keeps Sevak's own updater | `ManagedBy` marker tested | S2 |
| UPD-6 | L | AUR install (when published) | Same: self-update off | Marker tested | S3 |
| UPD-7 | all | Beta channel (when present): switch to Beta in Settings, check for updates, then switch back to Stable | Beta builds are offered only on Beta; going back to Stable does not offer a downgrade without asking and never loses config | Not yet testable | S2 |

### Diagnostics report (when present)

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| DIA-1 **S** | all | Use clipboard history, snippets and search, then generate a diagnostics report (wherever the feature puts it) | A report is produced locally; read it fully before sharing | Redaction function: unit tests | S1 |
| DIA-2 | all | Search the report for your user name, home path, a copied text, a snippet body, a query, a bookmark title, an email address | None of them appears; paths use placeholders | Unit-tested | S1 |
| DIA-3 | all | Check that nothing is sent anywhere by creating the report | It is a local file or clipboard text you share yourself | None | S1 |

### Network and privacy

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| NET-1 **S** | all | With default settings, run Sevak for 10 minutes with a network monitor (Wireshark, Little Snitch, Resource Monitor) while searching | Only the update check (GitHub); no other connection | No network in tests by rule | S1 |
| NET-2 | all | Block all network access | Every feature except updates, the gallery and currency works; nothing hangs | None | S1 |
| NET-3 | all | Open Settings and the gallery pages without clicking Load | No request | None | S1 |

### Accessibility

Status is honest: items marked **Unknown** have never been assessed and need a
first pass; record the result and file bugs.

| ID | OS | Steps | Expected | Auto | Status | Sev |
|---|---|---|---|---|---|---|
| A11-1 **S** | all | Use Sevak without a mouse: open with the shortcut, search, run, open the action panel, open Settings with `sevak --settings`, tab through every Settings page | Everything reachable by keyboard; focus always visible; no keyboard trap | Contrast and ARIA basics only | Partly known | S2 |
| A11-2 | W | Windows High Contrast on; and a dark/light built-in theme | Text and selection readable; the built-in themes keep text at WCAG AA contrast | `themes.test.ts` checks AA for built-ins; High Contrast mode itself is untested | **Unknown** | S2 |
| A11-3 | W | NVDA or Narrator: type a query and arrow through results | The selected result is announced (the search box uses `aria-activedescendant` over a listbox) | None | **Unknown** | S2 |
| A11-4 | M | VoiceOver: same | Same | None | **Unknown** | S2 |
| A11-5 | L | Orca: same | Same | None | **Unknown** | S2 |
| A11-6 | all | Increase the font size to the maximum in Settings; OS text scaling at 150 percent | No clipped text; window grows or scrolls | None | **Unknown** | S3 |
| A11-7 | all | Reduce motion in the OS | Animations stop (Sevak's own transitions honour `prefers-reduced-motion`) | None | Partly known | S4 |
| A11-8 | all | Settings: every control has a label (switches announce on/off) | Labels present | `Toggle.test.ts` for the switch | Partly known | S3 |

### Performance sanity

| ID | OS | Steps | Expected | Auto | Sev |
|---|---|---|---|---|---|
| PRF-1 **S** | all | Leave Sevak idle for 10 minutes and look at it in Task Manager, Activity Monitor or `top` | Close to 0 percent CPU; memory stable and modest | None | S2 |
| PRF-2 | all | Open the launcher 50 times | Appears promptly each time; memory does not grow | None | S2 |
| PRF-3 | all | With a large home folder (100,000+ files) restart Sevak, search while it indexes | Typing stays smooth; results complete when indexing finishes (`Reload index` shows it) | Latency budget over a 100,000-entry mock index | S2 |

## Keeping this page current

When a feature is added or a platform quirk is found, add a row with a new ID in
the right table and say what, if anything, already tests it. When a manual row
becomes automatic, change its **Auto** note and consider dropping it from the
smoke test. Rows are never renumbered, so reports from older releases stay
meaningful.
