# ADR-0007: Private browsing as per-window ephemeral WebKit stores

Status: accepted (M5)

## Context

Private browsing must avoid persisting cookies, caches, local storage, history
or session state. WebKit offers `WKWebsiteDataStore.nonPersistentDataStore`.
Engines create a `WKWebViewConfiguration` each, so a naive implementation gives
every tab its own isolated store, breaking login/session continuity inside a
private window.

## Decision

- A private window creates one non-persistent `WKWebsiteDataStore` at window
  creation and passes it to every tab engine (`EngineOptions.data_store`).
- The engine API exposes an opaque `PrivateDataStore`; only the macOS adapter can
  downcast it to `WKWebsiteDataStore`. No WebKit type leaks into UI/core.
- Private windows skip history recording and session persistence entirely.
- Each private window has its own store: two private windows do not share
  cookies. This is intentionally stricter than Safari's shared private session.
- The tab strip shows a "Private" badge and the window title is suffixed.

## Consequences

- Private tabs in one window behave like one browser session.
- Cookies disappear when the window closes; nothing is written to session or
  history.
- Bookmarks/settings remain shared user data, as in other browsers.
- A crashed app loses private session state entirely (desired).

## Verification

`m5-probe` opens one normal and two private windows and asserts store isolation,
intra-window cookie sharing, inter-window private isolation, and that private
visits do not appear in history or the saved session.
