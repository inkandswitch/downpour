// The Patchwork module: a "rain diagram" datatype, and the collaborative
// editor as its tool. Patchwork reads this in a worker, so it holds only
// metadata; the code comes in through each `load()`.

export const plugins = [
  {
    type: "patchwork:datatype",
    id: "rain-diagram",
    name: "Rain diagram",
    icon: "CloudRain",
    async load() {
      return (await import("./src/datatype.js")).RainDiagram;
    },
  },
  {
    type: "patchwork:tool",
    id: "rain-diagram",
    name: "Rain",
    icon: "CloudRain",
    supportedDatatypes: ["rain-diagram"],
    async load() {
      return (await import("./src/tool.js")).default;
    },
  },
];
