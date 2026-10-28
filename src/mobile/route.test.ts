import { describe, expect, it } from "vitest";
import { deviceName, isIos } from "./device";
import { parseRoute, routeHash, tabOf, type Route } from "./route";

describe("routes", () => {
  const routes: Route[] = [
    { name: "forYou" },
    { name: "projects" },
    { name: "project", id: "ws-1" },
    { name: "pane", id: "pane 7/b" },
    { name: "shares" },
    { name: "manager" },
    { name: "history" },
  ];

  it("read back what they write", () => {
    for (const route of routes) expect(parseRoute(routeHash(route))).toEqual(route);
  });

  it("open For you on anything unknown or empty", () => {
    expect(parseRoute("")).toEqual({ name: "forYou" });
    expect(parseRoute("#/nowhere")).toEqual({ name: "forYou" });
    expect(parseRoute("#/pane/")).toEqual({ name: "projects" });
  });

  it("light the tab each screen belongs to", () => {
    expect(tabOf({ name: "pane", id: "x" })).toBe("projects");
    expect(tabOf({ name: "project", id: "x" })).toBe("projects");
    expect(tabOf({ name: "history" })).toBe("forYou");
    expect(tabOf({ name: "manager" })).toBe("manager");
  });
});

describe("the device", () => {
  const IPHONE =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
  const IPHONE_CHROME =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/130.0.6723.90 Mobile/15E148 Safari/604.1";
  const IPAD_DESKTOP =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
  const PIXEL =
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36";
  const REDUCED =
    "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36";
  const SAMSUNG =
    "Mozilla/5.0 (Linux; Android 14; SM-S921B Build/UP1A.231005.007) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36";

  it("pairs under a name the PC's list can tell apart", () => {
    expect(deviceName(IPHONE)).toBe("iPhone (Safari)");
    expect(deviceName(IPHONE_CHROME)).toBe("iPhone (Chrome)");
    expect(deviceName(IPAD_DESKTOP, 5)).toBe("iPad (Safari)");
    expect(deviceName(IPAD_DESKTOP, 0)).toBe("Mac (Safari)");
    expect(deviceName(PIXEL)).toBe("Pixel 8 (Chrome)");
    expect(deviceName(REDUCED)).toBe("Android (Chrome)");
    expect(deviceName(SAMSUNG)).toBe("SM-S921B (Samsung Internet)");
  });

  it("knows an iPhone or an iPad, whatever it claims to be", () => {
    expect(isIos(IPHONE)).toBe(true);
    expect(isIos(IPAD_DESKTOP, 5)).toBe(true);
    expect(isIos(IPAD_DESKTOP, 0)).toBe(false);
    expect(isIos(PIXEL)).toBe(false);
  });
});
