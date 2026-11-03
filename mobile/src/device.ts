/**
 * The phone itself: the name it pairs under, as the PC's device list shows
 * it, and whether it is an iPhone that has yet to add the app to its Home
 * Screen.
 */

/** "iPhone (Safari)", "Pixel 8 (Chrome)": what the PC lists this device as. */
export function deviceName(userAgent: string, maxTouchPoints = 0): string {
  let device = "Browser";
  if (/iPhone|iPod/.test(userAgent)) device = "iPhone";
  else if (/iPad/.test(userAgent) || (/Macintosh/.test(userAgent) && maxTouchPoints > 1)) device = "iPad";
  else if (/Android/.test(userAgent)) {
    // Reduced user agents say "Android 10; K" whatever the phone.
    const model = /Android [\d.]+; ([^;)]+?)(?: Build\/[^;)]*)?[;)]/.exec(userAgent)?.[1]?.trim();
    device = model && model !== "K" ? model : "Android";
  } else if (/Windows/.test(userAgent)) device = "Windows";
  else if (/Macintosh/.test(userAgent)) device = "Mac";

  const browser = /EdgA|EdgiOS|Edg\//.test(userAgent)
    ? "Edge"
    : /FxiOS|Firefox\//.test(userAgent)
      ? "Firefox"
      : /SamsungBrowser/.test(userAgent)
        ? "Samsung Internet"
        : /CriOS|Chrome\//.test(userAgent)
          ? "Chrome"
          : /Safari\//.test(userAgent)
            ? "Safari"
            : null;
  return browser ? `${device} (${browser})` : device;
}

export function isIos(userAgent: string, maxTouchPoints = 0): boolean {
  return /iPhone|iPad|iPod/.test(userAgent) || (/Macintosh/.test(userAgent) && maxTouchPoints > 1);
}

/** Opened from the Home Screen rather than in a browser tab. */
export function isStandalone(): boolean {
  const legacy = (navigator as Navigator & { standalone?: boolean }).standalone;
  return legacy === true || matchMedia("(display-mode: standalone)").matches;
}
