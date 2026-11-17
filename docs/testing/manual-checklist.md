# Manual verification checklist

Run the app with `./scripts/cargo run -p browser-app --bin gpui-browser`.
Check each item and note the date/commit. Items marked *auto* are also covered
by `scripts/m0-probe.sh`.

## Window and chrome

- [ ] App launches to a native window with a visible tab strip and toolbar.
- [ ] Traffic-light buttons sit inside the tab strip without overlapping tabs.
- [ ] Dragging the empty tab-strip area moves the window.
- [ ] Window can be resized, minimized, zoomed and closed normally.
- [ ] Chrome follows the system light/dark appearance.
- [ ] Page content starts exactly below the chrome (no overlap, no gap).

## Navigation

- [ ] *auto* HTTPS site renders (text, images, CSS).
- [ ] *auto* JavaScript runs (e.g. a page with dynamic content).
- [ ] *auto* Enter in the address bar navigates to typed URLs (m1-probe).
- [ ] Typing a phrase performs a search with the configured engine.
- [ ] Back and forward buttons work; disabled states are correct.
- [ ] Reload refreshes the page; stop halts a slow load (`/slow` fixture).
- [ ] Failed navigation (e.g. `https://nonexistent.invalid`) shows an error page.
- [ ] Page title updates in the tab and window title.
- [ ] URL in the address bar updates after redirects and in-page navigation.

## Input

- [ ] *auto* Typing reaches page content (m1-probe, keydown fixture).
- [ ] Clicking a page input and typing works, including non-ASCII/IME.
- [ ] Trackpad and mouse-wheel scrolling works.
- [ ] Links and form submissions work.
- [ ] Keyboard shortcuts work while the page is focused: Cmd+L, Cmd+T, Cmd+W,
      Cmd+R, Cmd+[ , Cmd+] .
- [ ] Cmd+C / Cmd+V / Cmd+A work in page inputs and in the address bar.

## Tabs (when implemented)

- [ ] New tab, close tab, switch tab, reorder tab.
- [ ] Closing the last tab opens a new tab page (or closes the window, per spec).
- [ ] Session restores after quit/relaunch.
- [ ] A crashed web process shows a recoverable error, not a browser crash.

## Privacy (when implemented)

- [ ] Private window uses ephemeral storage (cookies gone after close).
- [ ] Permission prompts appear for camera/mic/location and denial works.
- [ ] Clear browsing data removes history, cookies and cache.

Record failures with app logs (`target/gpui-browser.log`), screenshot if
possible, and the exact URL.
