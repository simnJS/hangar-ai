import type { ITheme } from "@xterm/xterm";
import {
  contrastRatio,
  ensureContrast,
  hueAndSaturation,
  hueDistance,
  mix,
  toRgb,
  withHue,
} from "./lib/contrast";

export interface TerminalTheme {
  id: string;
  name: string;
  appearance: "dark" | "light";
  /** Drives the app chrome (active pane ring, buttons, focus states). */
  accent: string;
  xterm: ITheme;
}

export const THEMES: TerminalTheme[] = [
  {
    // Hangar's own: near-monochrome graphite, so that colour only ever means
    // something — an agent's state, a diff, the focus.
    id: "graphite",
    name: "Graphite",
    appearance: "dark",
    accent: "#3e8bff",
    xterm: {
      background: "#121316",
      foreground: "#c8c9ce",
      cursor: "#d8d9dd",
      cursorAccent: "#121316",
      selectionBackground: "#2a3954",
      black: "#1b1c1f",
      red: "#ff7b72",
      green: "#5fd27f",
      yellow: "#e7c06b",
      blue: "#79b0ff",
      magenta: "#c79bff",
      cyan: "#6fd3df",
      white: "#c8c9ce",
      brightBlack: "#6b6c73",
      brightRed: "#ff9a92",
      brightGreen: "#7fe29b",
      brightYellow: "#f2d38b",
      brightBlue: "#9cc4ff",
      brightMagenta: "#d8b6ff",
      brightCyan: "#93e3ec",
      brightWhite: "#ecedf0",
    },
  },
  {
    id: "tokyo-night",
    name: "Tokyo Night",
    appearance: "dark",
    accent: "#7aa2f7",
    xterm: {
      background: "#1a1b26",
      foreground: "#c0caf5",
      cursor: "#c0caf5",
      cursorAccent: "#1a1b26",
      selectionBackground: "#283457",
      black: "#15161e",
      red: "#f7768e",
      green: "#9ece6a",
      yellow: "#e0af68",
      blue: "#7aa2f7",
      magenta: "#bb9af7",
      cyan: "#7dcfff",
      white: "#a9b1d6",
      brightBlack: "#414868",
      brightRed: "#ff7a93",
      brightGreen: "#b9f27c",
      brightYellow: "#ff9e64",
      brightBlue: "#7da6ff",
      brightMagenta: "#bb9af7",
      brightCyan: "#0db9d7",
      brightWhite: "#c0caf5",
    },
  },
  {
    id: "catppuccin-mocha",
    name: "Catppuccin Mocha",
    appearance: "dark",
    accent: "#cba6f7",
    xterm: {
      background: "#1e1e2e",
      foreground: "#cdd6f4",
      cursor: "#f5e0dc",
      cursorAccent: "#1e1e2e",
      selectionBackground: "#585b70",
      black: "#45475a",
      red: "#f38ba8",
      green: "#a6e3a1",
      yellow: "#f9e2af",
      blue: "#89b4fa",
      magenta: "#f5c2e7",
      cyan: "#94e2d5",
      white: "#bac2de",
      brightBlack: "#585b70",
      brightRed: "#f38ba8",
      brightGreen: "#a6e3a1",
      brightYellow: "#f9e2af",
      brightBlue: "#89b4fa",
      brightMagenta: "#f5c2e7",
      brightCyan: "#94e2d5",
      brightWhite: "#a6adc8",
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    appearance: "dark",
    accent: "#bd93f9",
    xterm: {
      background: "#282a36",
      foreground: "#f8f8f2",
      cursor: "#f8f8f2",
      cursorAccent: "#282a36",
      selectionBackground: "#44475a",
      black: "#21222c",
      red: "#ff5555",
      green: "#50fa7b",
      yellow: "#f1fa8c",
      blue: "#bd93f9",
      magenta: "#ff79c6",
      cyan: "#8be9fd",
      white: "#f8f8f2",
      brightBlack: "#6272a4",
      brightRed: "#ff6e6e",
      brightGreen: "#69ff94",
      brightYellow: "#ffffa5",
      brightBlue: "#d6acff",
      brightMagenta: "#ff92df",
      brightCyan: "#a4ffff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "one-dark",
    name: "One Dark",
    appearance: "dark",
    accent: "#61afef",
    xterm: {
      background: "#282c34",
      foreground: "#abb2bf",
      cursor: "#528bff",
      cursorAccent: "#282c34",
      selectionBackground: "#3e4451",
      black: "#282c34",
      red: "#e06c75",
      green: "#98c379",
      yellow: "#e5c07b",
      blue: "#61afef",
      magenta: "#c678dd",
      cyan: "#56b6c2",
      white: "#abb2bf",
      brightBlack: "#5c6370",
      brightRed: "#e06c75",
      brightGreen: "#98c379",
      brightYellow: "#e5c07b",
      brightBlue: "#61afef",
      brightMagenta: "#c678dd",
      brightCyan: "#56b6c2",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "nord",
    name: "Nord",
    appearance: "dark",
    accent: "#88c0d0",
    xterm: {
      background: "#2e3440",
      foreground: "#d8dee9",
      cursor: "#d8dee9",
      cursorAccent: "#2e3440",
      selectionBackground: "#434c5e",
      black: "#3b4252",
      red: "#bf616a",
      green: "#a3be8c",
      yellow: "#ebcb8b",
      blue: "#81a1c1",
      magenta: "#b48ead",
      cyan: "#88c0d0",
      white: "#e5e9f0",
      brightBlack: "#4c566a",
      brightRed: "#bf616a",
      brightGreen: "#a3be8c",
      brightYellow: "#ebcb8b",
      brightBlue: "#81a1c1",
      brightMagenta: "#b48ead",
      brightCyan: "#8fbcbb",
      brightWhite: "#eceff4",
    },
  },
  {
    id: "gruvbox-dark",
    name: "Gruvbox Dark",
    appearance: "dark",
    accent: "#fabd2f",
    xterm: {
      background: "#282828",
      foreground: "#ebdbb2",
      cursor: "#ebdbb2",
      cursorAccent: "#282828",
      selectionBackground: "#504945",
      black: "#282828",
      red: "#cc241d",
      green: "#98971a",
      yellow: "#d79921",
      blue: "#458588",
      magenta: "#b16286",
      cyan: "#689d6a",
      white: "#a89984",
      brightBlack: "#928374",
      brightRed: "#fb4934",
      brightGreen: "#b8bb26",
      brightYellow: "#fabd2f",
      brightBlue: "#83a598",
      brightMagenta: "#d3869b",
      brightCyan: "#8ec07c",
      brightWhite: "#ebdbb2",
    },
  },
  {
    id: "github-dark",
    name: "GitHub Dark",
    appearance: "dark",
    accent: "#58a6ff",
    xterm: {
      background: "#0d1117",
      foreground: "#c9d1d9",
      cursor: "#58a6ff",
      cursorAccent: "#0d1117",
      selectionBackground: "#263b5e",
      black: "#484f58",
      red: "#ff7b72",
      green: "#3fb950",
      yellow: "#d29922",
      blue: "#58a6ff",
      magenta: "#bc8cff",
      cyan: "#39c5cf",
      white: "#b1bac4",
      brightBlack: "#6e7681",
      brightRed: "#ffa198",
      brightGreen: "#56d364",
      brightYellow: "#e3b341",
      brightBlue: "#79c0ff",
      brightMagenta: "#d2a8ff",
      brightCyan: "#56d4dd",
      brightWhite: "#f0f6fc",
    },
  },
  {
    id: "ayu-dark",
    name: "Ayu Dark",
    appearance: "dark",
    accent: "#e6b450",
    xterm: {
      background: "#0b0e14",
      foreground: "#bfbdb6",
      cursor: "#e6b450",
      cursorAccent: "#0b0e14",
      selectionBackground: "#22272d",
      black: "#131721",
      red: "#ea6c73",
      green: "#91b362",
      yellow: "#f9af4f",
      blue: "#53bdfa",
      magenta: "#d2a6ff",
      cyan: "#90e1c6",
      white: "#c7c7c7",
      brightBlack: "#686868",
      brightRed: "#f07178",
      brightGreen: "#c2d94c",
      brightYellow: "#ffb454",
      brightBlue: "#59c2ff",
      brightMagenta: "#ffee99",
      brightCyan: "#95e6cb",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "rose-pine",
    name: "Rosé Pine",
    appearance: "dark",
    accent: "#c4a7e7",
    xterm: {
      background: "#191724",
      foreground: "#e0def4",
      cursor: "#e0def4",
      cursorAccent: "#191724",
      selectionBackground: "#403d52",
      black: "#26233a",
      red: "#eb6f92",
      green: "#31748f",
      yellow: "#f6c177",
      blue: "#9ccfd8",
      magenta: "#c4a7e7",
      cyan: "#ebbcba",
      white: "#e0def4",
      brightBlack: "#6e6a86",
      brightRed: "#eb6f92",
      brightGreen: "#31748f",
      brightYellow: "#f6c177",
      brightBlue: "#9ccfd8",
      brightMagenta: "#c4a7e7",
      brightCyan: "#ebbcba",
      brightWhite: "#e0def4",
    },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    appearance: "dark",
    accent: "#2aa198",
    xterm: {
      background: "#002b36",
      foreground: "#839496",
      cursor: "#93a1a1",
      cursorAccent: "#002b36",
      selectionBackground: "#073642",
      black: "#073642",
      red: "#dc322f",
      green: "#859900",
      yellow: "#b58900",
      blue: "#268bd2",
      magenta: "#d33682",
      cyan: "#2aa198",
      white: "#eee8d5",
      brightBlack: "#586e75",
      brightRed: "#cb4b16",
      brightGreen: "#93a1a1",
      brightYellow: "#657b83",
      brightBlue: "#839496",
      brightMagenta: "#6c71c4",
      brightCyan: "#93a1a1",
      brightWhite: "#fdf6e3",
    },
  },
  {
    id: "vscode-dark",
    name: "VS Code Dark+",
    appearance: "dark",
    accent: "#0078d4",
    xterm: {
      background: "#1e1e1e",
      foreground: "#cccccc",
      cursor: "#ffffff",
      cursorAccent: "#1e1e1e",
      selectionBackground: "#264f78",
      black: "#000000",
      red: "#cd3131",
      green: "#0dbc79",
      yellow: "#e5e510",
      blue: "#2472c8",
      magenta: "#bc3fbc",
      cyan: "#11a8cd",
      white: "#e5e5e5",
      brightBlack: "#666666",
      brightRed: "#f14c4c",
      brightGreen: "#23d18b",
      brightYellow: "#f5f543",
      brightBlue: "#3b8eea",
      brightMagenta: "#d670d6",
      brightCyan: "#29b8db",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "night-owl",
    name: "Night Owl",
    appearance: "dark",
    accent: "#82aaff",
    xterm: {
      background: "#011627",
      foreground: "#d6deeb",
      cursor: "#80a4c2",
      cursorAccent: "#011627",
      selectionBackground: "#1d3b53",
      black: "#011627",
      red: "#ef5350",
      green: "#22da6e",
      yellow: "#c5e478",
      blue: "#82aaff",
      magenta: "#c792ea",
      cyan: "#21c7a8",
      white: "#ffffff",
      brightBlack: "#575656",
      brightRed: "#ef5350",
      brightGreen: "#22da6e",
      brightYellow: "#ffeb95",
      brightBlue: "#82aaff",
      brightMagenta: "#c792ea",
      brightCyan: "#7fdbca",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "monokai-pro",
    name: "Monokai Pro",
    appearance: "dark",
    accent: "#ffd866",
    xterm: {
      background: "#2d2a2e",
      foreground: "#fcfcfa",
      cursor: "#fcfcfa",
      cursorAccent: "#2d2a2e",
      selectionBackground: "#5b595c",
      black: "#403e41",
      red: "#ff6188",
      green: "#a9dc76",
      yellow: "#ffd866",
      blue: "#fc9867",
      magenta: "#ab9df2",
      cyan: "#78dce8",
      white: "#fcfcfa",
      brightBlack: "#727072",
      brightRed: "#ff6188",
      brightGreen: "#a9dc76",
      brightYellow: "#ffd866",
      brightBlue: "#fc9867",
      brightMagenta: "#ab9df2",
      brightCyan: "#78dce8",
      brightWhite: "#fcfcfa",
    },
  },
  {
    id: "material-palenight",
    name: "Material Palenight",
    appearance: "dark",
    accent: "#c792ea",
    xterm: {
      background: "#292d3e",
      foreground: "#a6accd",
      cursor: "#ffcc00",
      cursorAccent: "#292d3e",
      selectionBackground: "#444267",
      black: "#292d3e",
      red: "#f07178",
      green: "#c3e88d",
      yellow: "#ffcb6b",
      blue: "#82aaff",
      magenta: "#c792ea",
      cyan: "#89ddff",
      white: "#d0d0d0",
      brightBlack: "#434758",
      brightRed: "#ff8b92",
      brightGreen: "#ddffa7",
      brightYellow: "#ffe585",
      brightBlue: "#9cc4ff",
      brightMagenta: "#ddb0f6",
      brightCyan: "#a3f7ff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "synthwave-84",
    name: "SynthWave '84",
    appearance: "dark",
    accent: "#ff7edb",
    xterm: {
      background: "#262335",
      foreground: "#f8f8f2",
      cursor: "#ff7edb",
      cursorAccent: "#262335",
      selectionBackground: "#463465",
      black: "#2a2139",
      red: "#fe4450",
      green: "#72f1b8",
      yellow: "#fede5d",
      blue: "#03edf9",
      magenta: "#ff7edb",
      cyan: "#36f9f6",
      white: "#f8f8f2",
      brightBlack: "#495495",
      brightRed: "#fe4450",
      brightGreen: "#72f1b8",
      brightYellow: "#fede5d",
      brightBlue: "#03edf9",
      brightMagenta: "#ff7edb",
      brightCyan: "#36f9f6",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "cobalt2",
    name: "Cobalt2",
    appearance: "dark",
    accent: "#ffc600",
    xterm: {
      background: "#193549",
      foreground: "#ffffff",
      cursor: "#ffc600",
      cursorAccent: "#193549",
      selectionBackground: "#0050a4",
      black: "#000000",
      red: "#ff628c",
      green: "#3ad900",
      yellow: "#ffc600",
      blue: "#0088ff",
      magenta: "#fb94ff",
      cyan: "#80fcff",
      white: "#ffffff",
      brightBlack: "#5a7d94",
      brightRed: "#ff628c",
      brightGreen: "#3ad900",
      brightYellow: "#ffc600",
      brightBlue: "#0088ff",
      brightMagenta: "#fb94ff",
      brightCyan: "#80fcff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "shades-of-purple",
    name: "Shades of Purple",
    appearance: "dark",
    accent: "#fad000",
    xterm: {
      background: "#2d2b55",
      foreground: "#ffffff",
      cursor: "#fad000",
      cursorAccent: "#2d2b55",
      selectionBackground: "#4d21fc",
      black: "#1e1e3f",
      red: "#ff628c",
      green: "#3ad900",
      yellow: "#fad000",
      blue: "#40c4ff",
      magenta: "#fb94ff",
      cyan: "#80fcff",
      white: "#ffffff",
      brightBlack: "#a599e9",
      brightRed: "#ff628c",
      brightGreen: "#3ad900",
      brightYellow: "#fad000",
      brightBlue: "#9effff",
      brightMagenta: "#fb94ff",
      brightCyan: "#80fcff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "andromeda",
    name: "Andromeda",
    appearance: "dark",
    accent: "#00e8c6",
    xterm: {
      background: "#23262e",
      foreground: "#d5ced9",
      cursor: "#d5ced9",
      cursorAccent: "#23262e",
      selectionBackground: "#3d4352",
      black: "#000000",
      red: "#ee5d43",
      green: "#96e072",
      yellow: "#ffe66d",
      blue: "#00e8c6",
      magenta: "#c74ded",
      cyan: "#24c0cf",
      white: "#d5ced9",
      brightBlack: "#666666",
      brightRed: "#f92672",
      brightGreen: "#96e072",
      brightYellow: "#ffe66d",
      brightBlue: "#00e8c6",
      brightMagenta: "#c74ded",
      brightCyan: "#24c0cf",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "ayu-mirage",
    name: "Ayu Mirage",
    appearance: "dark",
    accent: "#ffcc66",
    xterm: {
      background: "#1f2430",
      foreground: "#cccac2",
      cursor: "#ffcc66",
      cursorAccent: "#1f2430",
      selectionBackground: "#33415e",
      black: "#191e2a",
      red: "#ed8274",
      green: "#87d96c",
      yellow: "#facc6e",
      blue: "#6dcbfa",
      magenta: "#dabafa",
      cyan: "#90e1c6",
      white: "#c7c7c7",
      brightBlack: "#686868",
      brightRed: "#f28779",
      brightGreen: "#d5ff80",
      brightYellow: "#ffd173",
      brightBlue: "#73d0ff",
      brightMagenta: "#dfbfff",
      brightCyan: "#95e6cb",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "horizon",
    name: "Horizon",
    appearance: "dark",
    accent: "#e95678",
    xterm: {
      background: "#1c1e26",
      foreground: "#d5d8da",
      cursor: "#e95678",
      cursorAccent: "#1c1e26",
      selectionBackground: "#2e303e",
      black: "#16161c",
      red: "#e95678",
      green: "#29d398",
      yellow: "#fab795",
      blue: "#26bbd9",
      magenta: "#ee64ac",
      cyan: "#59e3e3",
      white: "#fadad1",
      brightBlack: "#3d3f4c",
      brightRed: "#ec6a88",
      brightGreen: "#3fdaa4",
      brightYellow: "#fbc3a7",
      brightBlue: "#3fc6de",
      brightMagenta: "#f075b7",
      brightCyan: "#6be6e6",
      brightWhite: "#fdf0ed",
    },
  },
  {
    id: "panda",
    name: "Panda",
    appearance: "dark",
    accent: "#19f9d8",
    xterm: {
      background: "#292a2b",
      foreground: "#e6e6e6",
      cursor: "#ff2c6d",
      cursorAccent: "#292a2b",
      selectionBackground: "#414243",
      black: "#292a2b",
      red: "#ff2c6d",
      green: "#19f9d8",
      yellow: "#ffb86c",
      blue: "#45a9f9",
      magenta: "#ff75b5",
      cyan: "#6fc1ff",
      white: "#e6e6e6",
      brightBlack: "#676b79",
      brightRed: "#ff2c6d",
      brightGreen: "#19f9d8",
      brightYellow: "#ffcc95",
      brightBlue: "#6fc1ff",
      brightMagenta: "#ff9ac1",
      brightCyan: "#b084eb",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "kanagawa",
    name: "Kanagawa",
    appearance: "dark",
    accent: "#7e9cd8",
    xterm: {
      background: "#1f1f28",
      foreground: "#dcd7ba",
      cursor: "#c8c093",
      cursorAccent: "#1f1f28",
      selectionBackground: "#2d4f67",
      black: "#090618",
      red: "#c34043",
      green: "#76946a",
      yellow: "#c0a36e",
      blue: "#7e9cd8",
      magenta: "#957fb8",
      cyan: "#6a9589",
      white: "#c8c093",
      brightBlack: "#727169",
      brightRed: "#e82424",
      brightGreen: "#98bb6c",
      brightYellow: "#e6c384",
      brightBlue: "#7fb4ca",
      brightMagenta: "#938aa9",
      brightCyan: "#7aa89f",
      brightWhite: "#dcd7ba",
    },
  },
  {
    id: "everforest-dark",
    name: "Everforest Dark",
    appearance: "dark",
    accent: "#a7c080",
    xterm: {
      background: "#2d353b",
      foreground: "#d3c6aa",
      cursor: "#d3c6aa",
      cursorAccent: "#2d353b",
      selectionBackground: "#475258",
      black: "#475258",
      red: "#e67e80",
      green: "#a7c080",
      yellow: "#dbbc7f",
      blue: "#7fbbb3",
      magenta: "#d699b6",
      cyan: "#83c092",
      white: "#d3c6aa",
      brightBlack: "#7a8478",
      brightRed: "#e67e80",
      brightGreen: "#a7c080",
      brightYellow: "#dbbc7f",
      brightBlue: "#7fbbb3",
      brightMagenta: "#d699b6",
      brightCyan: "#83c092",
      brightWhite: "#e6e2cc",
    },
  },
  {
    id: "poimandres",
    name: "Poimandres",
    appearance: "dark",
    accent: "#5de4c7",
    xterm: {
      background: "#1b1e28",
      foreground: "#a6accd",
      cursor: "#a6accd",
      cursorAccent: "#1b1e28",
      selectionBackground: "#303340",
      black: "#1b1e28",
      red: "#d0679d",
      green: "#5de4c7",
      yellow: "#fffac2",
      blue: "#89ddff",
      magenta: "#fcc5e9",
      cyan: "#add7ff",
      white: "#ffffff",
      brightBlack: "#767c9d",
      brightRed: "#d0679d",
      brightGreen: "#5de4c7",
      brightYellow: "#fffac2",
      brightBlue: "#add7ff",
      brightMagenta: "#fae4fc",
      brightCyan: "#89ddff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "catppuccin-latte",
    name: "Catppuccin Latte",
    appearance: "light",
    accent: "#8839ef",
    xterm: {
      background: "#eff1f5",
      foreground: "#4c4f69",
      cursor: "#dc8a78",
      cursorAccent: "#eff1f5",
      selectionBackground: "#bcc0cc",
      black: "#5c5f77",
      red: "#d20f39",
      green: "#40a02b",
      yellow: "#df8e1d",
      blue: "#1e66f5",
      magenta: "#ea76cb",
      cyan: "#179299",
      white: "#acb0be",
      brightBlack: "#6c6f85",
      brightRed: "#d20f39",
      brightGreen: "#40a02b",
      brightYellow: "#df8e1d",
      brightBlue: "#1e66f5",
      brightMagenta: "#ea76cb",
      brightCyan: "#179299",
      brightWhite: "#bcc0cc",
    },
  },
  {
    id: "github-light",
    name: "GitHub Light",
    appearance: "light",
    accent: "#0969da",
    xterm: {
      background: "#ffffff",
      foreground: "#24292f",
      cursor: "#0969da",
      cursorAccent: "#ffffff",
      selectionBackground: "#b6d7ff",
      black: "#24292f",
      red: "#cf222e",
      green: "#116329",
      yellow: "#4d2d00",
      blue: "#0969da",
      magenta: "#8250df",
      cyan: "#1b7c83",
      white: "#6e7781",
      brightBlack: "#57606a",
      brightRed: "#a40e26",
      brightGreen: "#1a7f37",
      brightYellow: "#633c01",
      brightBlue: "#218bff",
      brightMagenta: "#a475f9",
      brightCyan: "#3192aa",
      brightWhite: "#8c959f",
    },
  },
  {
    id: "one-light",
    name: "One Light",
    appearance: "light",
    accent: "#4078f2",
    xterm: {
      background: "#fafafa",
      foreground: "#383a42",
      cursor: "#526fff",
      cursorAccent: "#fafafa",
      selectionBackground: "#dbdbdc",
      black: "#383a42",
      red: "#e45649",
      green: "#50a14f",
      yellow: "#c18401",
      blue: "#4078f2",
      magenta: "#a626a4",
      cyan: "#0184bc",
      white: "#a0a1a7",
      brightBlack: "#4f525e",
      brightRed: "#e06c75",
      brightGreen: "#3f953a",
      brightYellow: "#986801",
      brightBlue: "#3f7cd8",
      brightMagenta: "#8f2f8d",
      brightCyan: "#0b6e99",
      brightWhite: "#c8c8c9",
    },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    appearance: "light",
    accent: "#268bd2",
    xterm: {
      background: "#fdf6e3",
      foreground: "#657b83",
      cursor: "#586e75",
      cursorAccent: "#fdf6e3",
      selectionBackground: "#eee8d5",
      black: "#073642",
      red: "#dc322f",
      green: "#859900",
      yellow: "#b58900",
      blue: "#268bd2",
      magenta: "#d33682",
      cyan: "#2aa198",
      white: "#93a1a1",
      brightBlack: "#002b36",
      brightRed: "#cb4b16",
      brightGreen: "#586e75",
      brightYellow: "#657b83",
      brightBlue: "#839496",
      brightMagenta: "#6c71c4",
      brightCyan: "#93a1a1",
      brightWhite: "#eee8d5",
    },
  },
  {
    id: "quiet-light",
    name: "Quiet Light",
    appearance: "light",
    accent: "#325cc0",
    xterm: {
      background: "#f5f5f5",
      foreground: "#333333",
      cursor: "#333333",
      cursorAccent: "#f5f5f5",
      selectionBackground: "#c9d0d9",
      black: "#333333",
      red: "#aa3731",
      green: "#448c27",
      yellow: "#cb9000",
      blue: "#325cc0",
      magenta: "#7a3e9d",
      cyan: "#0083b2",
      white: "#bbbbbb",
      brightBlack: "#777777",
      brightRed: "#c5544e",
      brightGreen: "#5aa63d",
      brightYellow: "#d9a520",
      brightBlue: "#4b83cd",
      brightMagenta: "#9a5fbb",
      brightCyan: "#2a9dc9",
      brightWhite: "#dddddd",
    },
  },
  {
    id: "ayu-light",
    name: "Ayu Light",
    appearance: "light",
    accent: "#f2ae49",
    xterm: {
      background: "#fcfcfc",
      foreground: "#5c6166",
      cursor: "#ffaa33",
      cursorAccent: "#fcfcfc",
      selectionBackground: "#dfeaf5",
      black: "#000000",
      red: "#ea6c6d",
      green: "#6cbf43",
      yellow: "#eca944",
      blue: "#3199e1",
      magenta: "#9e75c7",
      cyan: "#46ba94",
      white: "#c7c7c7",
      brightBlack: "#8a9199",
      brightRed: "#f07171",
      brightGreen: "#86b300",
      brightYellow: "#f2ae49",
      brightBlue: "#399ee6",
      brightMagenta: "#a37acc",
      brightCyan: "#4cbf99",
      brightWhite: "#d1d1d1",
    },
  },
  {
    id: "rose-pine-dawn",
    name: "Rosé Pine Dawn",
    appearance: "light",
    accent: "#907aa9",
    xterm: {
      background: "#faf4ed",
      foreground: "#575279",
      cursor: "#575279",
      cursorAccent: "#faf4ed",
      selectionBackground: "#dfdad9",
      black: "#f2e9e1",
      red: "#b4637a",
      green: "#286983",
      yellow: "#ea9d34",
      blue: "#56949f",
      magenta: "#907aa9",
      cyan: "#d7827e",
      white: "#575279",
      brightBlack: "#9893a5",
      brightRed: "#b4637a",
      brightGreen: "#286983",
      brightYellow: "#ea9d34",
      brightBlue: "#56949f",
      brightMagenta: "#907aa9",
      brightCyan: "#d7827e",
      brightWhite: "#575279",
    },
  },
  {
    id: "night-owl-light",
    name: "Night Owl Light",
    appearance: "light",
    accent: "#288ed7",
    xterm: {
      background: "#fbfbfb",
      foreground: "#403f53",
      cursor: "#90a7b2",
      cursorAccent: "#fbfbfb",
      selectionBackground: "#e0e0e0",
      black: "#403f53",
      red: "#de3d3b",
      green: "#08916a",
      yellow: "#daaa01",
      blue: "#288ed7",
      magenta: "#d6438a",
      cyan: "#2aa298",
      white: "#989fb1",
      brightBlack: "#403f53",
      brightRed: "#de3d3b",
      brightGreen: "#08916a",
      brightYellow: "#daaa01",
      brightBlue: "#288ed7",
      brightMagenta: "#d6438a",
      brightCyan: "#2aa298",
      brightWhite: "#f0f0f0",
    },
  },
];

export const getTheme = (id: string): TerminalTheme =>
  THEMES.find((t) => t.id === id) ?? THEMES[0];


export { mix };

export const withAlpha = (hex: string, alpha: number): string => {
  const [r, g, b] = toRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
};

/** Text on the chrome reads at this ratio at least: WCAG's AA for body text. */
export const TEXT_CONTRAST = 4.5;
/** A state's dot, a bar, a frame: WCAG's 3:1 for what is not text. */
export const FILL_CONTRAST = 3;

/** Two hues closer than this pass for one colour at a glance. */
const DISTINCT_HUE = 30;
/** "Your turn" stays somewhere from cyan to blue, whatever it steers clear of. */
const YOURS_HUES = { from: 165, to: 240 };

/**
 * The colour "your turn" starts from: the theme's cyan, or its blue, as long
 * as it reads as neither the accent — which marks links, focus and the
 * selection — nor working's green. A theme with no such colour has its cyan
 * turned to the nearest hue clear of both.
 */
function yoursBase(theme: TerminalTheme, working: string): string {
  const accentHue = hueAndSaturation(theme.accent).hue;
  const workingHue = hueAndSaturation(working).hue;
  const apart = (hue: number) =>
    Math.min(hueDistance(hue, accentHue), hueDistance(hue, workingHue));
  const { cyan, brightCyan, blue, brightBlue } = theme.xterm;
  // Some palettes fill these slots with a grey, or with what is really a
  // pink: only a cyan or a blue worth the name is a candidate.
  const candidates = [cyan, brightCyan, blue, brightBlue].filter((color): color is string => {
    if (!color) return false;
    const { hue, saturation } = hueAndSaturation(color);
    return saturation >= 0.25 && hue >= YOURS_HUES.from && hue <= YOURS_HUES.to;
  });
  const ready = candidates.find((color) => apart(hueAndSaturation(color).hue) >= DISTINCT_HUE);
  if (ready) return ready;

  const base = candidates[0] ?? "#3fb8cf";
  const baseHue = hueAndSaturation(base).hue;
  let best = baseHue;
  let bestScore = -Infinity;
  for (let hue = YOURS_HUES.from; hue <= YOURS_HUES.to; hue++) {
    // Clear of both, as near the theme's own hue as that allows; failing
    // that, as far from the nearer of the two as the range goes.
    const score = apart(hue) >= DISTINCT_HUE ? 1000 - hueDistance(hue, baseHue) : apart(hue);
    if (score > bestScore) {
      bestScore = score;
      best = hue;
    }
  }
  return withHue(base, best);
}

/**
 * Every colour token of the app chrome, derived from the terminal palette, so
 * switching a terminal theme restyles the entire window rather than just the
 * panes. Computed here rather than with light-dark() or color-mix() in the
 * stylesheets, so a token never depends on what the web view supports. Pure,
 * so a test can hold every bundled theme to the contrast it promises.
 */
export function deriveChromeTokens(theme: TerminalTheme): Record<string, string> {
  const bg = theme.xterm.background ?? "#1a1b26";
  const isDark = theme.appearance === "dark";
  const lift = isDark ? "#ffffff" : "#000000";

  /** A step from the terminal background toward the foreground side. */
  const lifted = (amount: number) => mix(bg, lift, amount);

  const chrome = lifted(0.04);
  const chromeBar = lifted(0.06);
  const raised = lifted(0.07);
  const paneHead = lifted(0.025);
  const bgSunken = mix(bg, isDark ? "#000000" : "#ffffff", 0.35);

  /**
   * Every surface the chrome writes on: the navigator, inspector and status
   * bar, the toolbar, raised groups and cards, pane headers. A colour is held
   * to the least favourable of them: on a dark theme the lightest, on a light
   * one the darkest.
   */
  const surfaces = [chrome, chromeBar, raised, paneHead];
  /**
   * A step further out: the selected row, and a hairline's tint laid over a
   * raised group or the toolbar (a chip, the activity capsule). Whatever can
   * be written there (body text, labels, state words, links, status colours)
   * clears those as well. The two quietest greys answer to the plain surfaces
   * only, so they stay a step below the labels; a rule that puts them on a
   * fill takes --fg-secondary instead.
   */
  const selection = mix(bg, lift, 0.13);
  const lineAlpha = isDark ? 0.07 : 0.08;
  const tinted = mix(raised, lift, lineAlpha);
  const anywhere = [...surfaces, selection, tinted];
  const text = (color: string, toward?: string) =>
    ensureContrast(color, anywhere, TEXT_CONTRAST, toward);
  const fill = (color: string) => ensureContrast(color, anywhere, FILL_CONTRAST);

  // The theme's foreground, unless it is too faint for the chrome (Solarized):
  // body text there leans out to white or black until it reads.
  const fg = text(theme.xterm.foreground ?? "#c0caf5");
  /**
   * Text a step back from the foreground: `amount` of the way to the
   * background, or nearer the foreground until it reads on `on`. Leaning on
   * the foreground rather than on white keeps the theme's tint in its greys.
   */
  const quieter = (amount: number, on: string[] = surfaces) =>
    ensureContrast(mix(fg, bg, amount), on, TEXT_CONTRAST, fg);

  // A light theme's "yellow" is often a dark brown that reads as body text,
  // so light themes start from an amber that still looks like one.
  const amber = isDark ? (theme.xterm.yellow ?? "#e7c06b") : "#b45309";
  const green = theme.xterm.green ?? "#5fd27f";
  const yours = yoursBase(theme, green);
  const danger = text(theme.xterm.red ?? "#f7768e");
  // Text on the accent (a primary button, a selected row): the accent darkened
  // almost to black on a dark theme, white on a light one, unless the accent
  // is too light or too dark for that to read; then the other one.
  const darkOnAccent = ensureContrast(
    mix(theme.accent, "#000000", 0.82),
    theme.accent,
    TEXT_CONTRAST,
    "#000000",
  );
  const [preferred, other] = isDark ? [darkOnAccent, "#ffffff"] : ["#ffffff", darkOnAccent];
  const onAccent =
    contrastRatio(preferred, theme.accent) >= TEXT_CONTRAST ||
    contrastRatio(preferred, theme.accent) >= contrastRatio(other, theme.accent)
      ? preferred
      : other;

  return {
    // The chrome around the terminals, in a few steps away from their
    // background: the navigator, inspector and status bar sit on `--chrome`,
    // the toolbar a step above, pane headers just off the terminal itself.
    "--chrome": chrome,
    "--chrome-bar": chromeBar,
    "--pane-head": paneHead,
    "--pane-head-focus": lifted(0.045),
    "--raised": raised,
    // A card, a settings group: lifted off the page on a dark theme, a white
    // sheet on the grey under it on a light one, the way the system draws it.
    "--card": isDark ? raised : bg,
    // What sits between panes and columns: darker than anything around it.
    "--sep": mix(bg, "#000000", isDark ? 0.5 : 0.14),
    "--line": withAlpha(lift, lineAlpha),
    "--line-strong": withAlpha(lift, isDark ? 0.12 : 0.14),
    "--accent-hi": text(isDark ? mix(theme.accent, "#ffffff", 0.22) : theme.accent),
    // An agent's state, always in these four, and always with its shape in
    // the UI. Each comes twice: the word, at 4.5:1 like any text, and the
    // `-fill` of its glyph, a bar or a frame, held only to 3:1 so it keeps
    // the theme's own vivid colour. "Your turn" has a hue of its own, so it
    // never reads as a link or as the focus.
    "--state-working": text(green),
    "--state-working-fill": fill(green),
    "--state-waiting": text(amber),
    "--state-waiting-fill": fill(amber),
    "--state-yours": text(yours),
    "--state-yours-fill": fill(yours),
    "--state-idle": quieter(0.55, anywhere),
    "--state-idle-fill": fill(mix(fg, bg, 0.55)),
    "--bg": bg,
    "--bg-sunken": bgSunken,
    "--surface": mix(bg, lift, 0.04),
    "--surface-hover": mix(bg, lift, 0.08),
    "--surface-active": selection,
    "--border": mix(bg, lift, 0.11),
    "--border-strong": mix(bg, lift, 0.2),
    "--fg": fg,
    // Secondary text in three steps (labels, then metadata, then the quietest
    // hints, paths and counts), each at 4.5:1 on every surface it is drawn
    // on, which on a low-contrast theme draws them closer together.
    "--fg-secondary": quieter(0.24, anywhere),
    "--fg-muted": quieter(0.33),
    "--fg-subtle": quieter(0.5),
    "--accent": theme.accent,
    "--accent-soft": withAlpha(theme.accent, 0.14),
    "--accent-ring": withAlpha(theme.accent, 0.45),
    "--accent-fg": onAccent,
    "--danger": danger,
    // Behind something destructive under the pointer, or a badge that warns.
    "--danger-soft": withAlpha(danger, 0.14),
    "--success": text(green),
    // Same amber as --state-waiting, for the same reason.
    "--warning": text(amber),
    "--shadow": isDark ? "rgba(0,0,0,0.45)" : "rgba(0,0,0,0.12)",
    // What floats over the window gets a dark outer edge under its light
    // hairline on a dark theme, so a panel never melts into what is behind
    // it; on a light theme the shadow does that almost alone.
    "--float-ring": isDark ? "rgba(0,0,0,0.55)" : "rgba(0,0,0,0.05)",
    // A segmented control: a recessed track, and the chosen segment raised
    // out of it — lighter on a dark theme, white on a light one. On a dark
    // theme of little contrast it rises only as far as its label still reads.
    "--seg-track": isDark ? bg : mix(bg, lift, 0.13),
    "--seg-thumb": isDark
      ? ensureContrast(mix(chromeBar, fg, 0.17), fg, TEXT_CONTRAST, chromeBar)
      : bgSunken,
  };
}

export function applyThemeToDocument(theme: TerminalTheme) {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(deriveChromeTokens(theme))) {
    root.style.setProperty(key, value);
  }
  root.style.colorScheme = theme.appearance;
}
