# ADR-0005: Favicons from page hints, fetched off the UI thread

Status: accepted (M2)

## Context

`WKWebView` exposes no favicon API. Browsers must discover the icon themselves:
the page's `<link rel="icon">` hint, or the implicit `/favicon.ico`. Network IO
must not block the GPUI main thread, and GPUI ships no default HTTP client
(`Application::new()` installs a `NullHttpClient`).

## Decision

1. After a successful main-frame load, evaluate a small expression in the page
   to find the declared icon URL (`link[rel~="icon"][href]`).
2. If no icon is declared, fall back to `<origin>/favicon.ico`.
3. Fetch on a dedicated worker thread with `ureq` (bounded to 512 KB, 4
   redirects, short timeouts, http(s) only). Send bytes back over an
   `async_channel`; decode with `image` on the UI thread.
4. Cache successful icons per origin; cache misses per page URL so a different
   page on the same origin still gets a chance.
5. Evidence: `m1-probe` verifies discovery + decode against a fixture page whose
   `/favicon.ico` is deliberately 404 and whose `<link rel="icon">` is the only
   source.

## Consequences

- No GPUI HttpClient implementation needed yet; future downloads/network
  features will introduce a proper `browser-network` crate.
- One worker thread per in-flight icon, deduplicated by page URL.
- SVG icons are unsupported by the `image` crate; pages using only SVG favicons
  show no icon. Documented, not silent.
- `ureq`, `image`, `smallvec` become direct app dependencies; all are already
  in the tree via GPUI or are small.

## Alternatives rejected

- Implementing `gpui::http_client::HttpClient` with reqwest: reqwest expects a
  Tokio reactor, while GPUI runs smol; a blocking worker is simpler and isolated.
- `WKWebView` private favicon APIs: unsupported and fragile.
- Fetching favicons in the page via JS/canvas: cross-origin/CSP issues and more
  page privileges than needed.
