# ADR-0006: Downloads through WKDownload with a policy-chosen destination

Status: accepted (M3)

## Context

The browser must save files without exposing arbitrary filesystem paths to
websites. WebKit offers `WKDownload` with a delegate that chooses a destination
and reports completion. It provides no progress API and no resume.

## Decision

1. Navigation policy returns `.Download` when:
   - the navigation action has `shouldPerformDownload` (e.g. download
     attribute), or
   - the response cannot be displayed by WebKit, or
   - the HTTP `Content-Disposition` header contains `attachment`.
2. The `WKDownloadDelegate` picks a destination inside the browser's downloads
   directory (overridable with `GPUI_BROWSER_DOWNLOADS_DIR`), sanitising the
   filename and resolving collisions with pure functions in
   `browser-security`.
3. The engine emits `DownloadStarted { url, filename, path, expected_bytes }`
   and `DownloadFinished { path, success, message }`. The UI samples file size
   for progress and calls `WKDownload.cancel` for cancellation.
4. Destinations are never taken from the page; only the suggested filename is
   used, and only after sanitisation.

## Consequences

- A malicious filename cannot escape the downloads directory.
- The UI has no per-second progress events; it polls metadata while downloading.
- Resume, authentication-protected downloads and quarantine attributes are not
  implemented; macOS still applies its own quarantine to downloaded files.
- Downloads are not persisted across restarts yet (download page shows the
  current session).

## Alternatives rejected

- Hand-rolled URLSession downloads: duplicates WebKit's cookie/auth/redirect
  handling and risks leaking sessions.
- Saving to the current working directory: unsafe and surprising.
