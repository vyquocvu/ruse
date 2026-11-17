# ADR-0004: One WKWebView (and web process) per tab

Status: accepted (M0)

## Context

Each tab needs an independent page context: DOM, JavaScript globals, history,
form state and lifecycle. WebKit's supported model is one WKWebView per page.
M0 measured two views in one window: two distinct WebContent XPC processes plus
shared Networking/GPU helpers (~150 MB peak for helpers with two views during
the probe).

## Decision

Each tab owns one engine instance (`WKWebView`) and is represented by a
`Rc<dyn Engine>` in the UI. Only the active tab's view is visible and positioned;
inactive views are hidden (`setHidden(true)`), not destroyed, so switching tabs
is instant.

## Consequences

- Tabs are memory-isolated at the process level (a page crash does not take the
  browser down; `webViewWebContentProcessDidTerminate` is reported).
- Memory grows with tab count; closing tabs must release engines eagerly.
  Inactive-tab hibernation is a future backlog item.
- Tab switching does not reload pages.

## Alternatives rejected

- One view + navigate on switch: destroys history/form state, slow.
- Shared process pool tricks: unsupported/private API.
