# ADR-0008: Deny device permissions by default until signed packaging

Status: accepted (M5), to be superseded by M6 prompt work

## Context

`WKUIDelegate` lets the app grant camera/microphone access. A real grant needs a
proper app bundle with `NSCameraUsageDescription`/`NSMicrophoneUsageDescription`
and a user-facing decision UI. The development binary is not bundled, and
granting from web content without those safeguards would be unsafe.

## Decision

- Media capture requests are denied in the delegate and logged.
- No other device capabilities (location, notifications) are exposed.
- Documentation and SECURITY.md state this plainly; the UI does not pretend a
  permission prompt exists.

## Consequences

- Video calls and microphone recording do not work in this build.
- The default is safe; no site can access devices.
- M6 packaging adds the bundle, usage strings, a GPUI permission sheet and
  per-origin remembered decisions, replacing this ADR.
