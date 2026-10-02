// Unit tests run in Node; present a macOS browser so shortcut display is deterministic.
Object.defineProperty(globalThis, "navigator", {
  value: { platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh)" },
  configurable: true,
});
