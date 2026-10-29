/**
 * Where the app is, in the address's hash: the browser's back gesture works,
 * and a notification can open a screen (#/pane/<id>, #/shares, #/stats, #/).
 */

export type Route =
  | { name: "forYou" }
  | { name: "projects" }
  | { name: "project"; id: string }
  | { name: "pane"; id: string }
  | { name: "shares" }
  | { name: "manager" }
  | { name: "history" }
  | { name: "stats" };

export type Tab = "forYou" | "projects" | "shares" | "manager" | "stats";

export function parseRoute(hash: string): Route {
  const [head, ...rest] = hash.replace(/^#\/?/, "").split("/");
  const id = rest.length ? decodeURIComponent(rest.join("/")) : "";
  switch (head) {
    case "projects":
      return id ? { name: "project", id } : { name: "projects" };
    case "pane":
      return id ? { name: "pane", id } : { name: "projects" };
    case "shares":
      return { name: "shares" };
    case "manager":
      return { name: "manager" };
    case "history":
      return { name: "history" };
    case "stats":
      return { name: "stats" };
    default:
      return { name: "forYou" };
  }
}

export function routeHash(route: Route): string {
  switch (route.name) {
    case "forYou":
      return "#/";
    case "project":
      return `#/projects/${encodeURIComponent(route.id)}`;
    case "pane":
      return `#/pane/${encodeURIComponent(route.id)}`;
    default:
      return `#/${route.name}`;
  }
}

/** The tab a screen belongs to: a pane is reached from a project or a card. */
export function tabOf(route: Route): Tab {
  switch (route.name) {
    case "projects":
    case "project":
    case "pane":
      return "projects";
    case "history":
      return "forYou";
    default:
      return route.name;
  }
}
