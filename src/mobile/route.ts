/**
 * Where the app is, in the address's hash: the browser's back gesture works,
 * and a notification can open a screen (#/pane/<id>, #/shares, #/stats, #/).
 * A delivery waiting to land opens on #/delivery/<task>, one of its files on
 * #/diff/<task>/<path>.
 */

export type Route =
  | { name: "forYou" }
  | { name: "projects" }
  | { name: "project"; id: string }
  | { name: "pane"; id: string }
  | { name: "delivery"; task: string }
  | { name: "diff"; task: string; file: string }
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
    case "delivery":
      return id ? { name: "delivery", task: id } : { name: "forYou" };
    case "diff": {
      // The task, then the file's path, each encoded whole.
      const [task, file] = rest.map(decodeURIComponent);
      return task && file ? { name: "diff", task, file } : task ? { name: "delivery", task } : { name: "forYou" };
    }
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
    case "delivery":
      return `#/delivery/${encodeURIComponent(route.task)}`;
    case "diff":
      return `#/diff/${encodeURIComponent(route.task)}/${encodeURIComponent(route.file)}`;
    default:
      return `#/${route.name}`;
  }
}

/** The tab a screen belongs to: a pane or a delivery is reached from a project or a card. */
export function tabOf(route: Route): Tab {
  switch (route.name) {
    case "projects":
    case "project":
    case "pane":
    case "delivery":
    case "diff":
      return "projects";
    case "history":
      return "forYou";
    default:
      return route.name;
  }
}
