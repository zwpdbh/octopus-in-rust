// Thin bridge between the Rust/Dioxus frontend and AntV G6.
// Exposes window.fafG6.init(containerId, json) and window.fafG6.destroy().

(function () {
  "use strict";

  function toG6Data(input) {
    const nodes = input.nodes.map(function (n) {
      return {
        id: n.id,
        label: n.label,
        color: n.color || "#f8f9fa",
        layer: n.layer,
        summary: n.summary,
      };
    });

    const idByIndex = input.nodes.map(function (n) {
      return n.id;
    });

    const edges = input.edges.map(function (e, i) {
      return {
        id: "e" + i,
        source: e.source,
        target: e.target,
        color: e.color || "#9ca3af",
        dashed: !!e.dashed,
      };
    });

    return { nodes, edges };
  }

  window.fafG6 = {
    graph: null,

    init: async function (containerId, jsonString) {
      this.destroy();

      const container = document.getElementById(containerId);
      if (!container) {
        console.error("[fafG6] container not found:", containerId);
        return;
      }

      // Wait one frame so the container has its final flex layout size.
      await new Promise(requestAnimationFrame);
      console.log("[fafG6] init in container", containerId, "size", container.clientWidth, container.clientHeight);

      let input;
      try {
        input = JSON.parse(jsonString);
      } catch (err) {
        console.error("[fafG6] failed to parse JSON:", err);
        return;
      }
      const data = toG6Data(input);
      console.log("[fafG6] data nodes:", data.nodes.length, "edges:", data.edges.length);

      try {
        const graph = new G6.Graph({
          container: container,
          autoFit: "view",
          autoResize: true,
          data: data,
          layout: {
            type: "antv-dagre",
            rankdir: "LR",
            ranksep: 120,
            nodesep: 50,
            edgesep: 20,
            align: "UL",
          },
          node: {
            type: "rect",
            style: function (d) {
              return {
                size: [140, 40],
                fill: d.color,
                stroke: "#333333",
                lineWidth: 1.5,
                radius: 6,
                labelText: d.label,
                labelFill: "#212529",
                labelFontSize: 12,
                labelPlacement: "center",
                labelMaxWidth: 130,
              };
            },
          },
          edge: {
            type: "cubic-horizontal",
            style: function (d) {
              return {
                stroke: d.color,
                lineWidth: 1.5,
                lineDash: d.dashed ? [4, 4] : [],
                endArrow: true,
                endArrowFill: d.color,
                endArrowSize: 10,
              };
            },
          },
          behaviors: ["drag-canvas", "zoom-canvas"],
        });

        graph.on("node:click", function (e) {
          const id = e.item && e.item.id;
          if (id) {
            document.dispatchEvent(
              new CustomEvent("faf:g6-node-click", { detail: id })
            );
          }
        });

        await graph.render();
        console.log("[fafG6] render complete");
        this.graph = graph;
      } catch (err) {
        console.error("[fafG6] failed to create/render graph:", err);
      }
    },

    destroy: function () {
      if (this.graph) {
        try {
          this.graph.destroy();
        } catch (err) {
          console.error("Failed to destroy G6 graph:", err);
        }
        this.graph = null;
      }
    },
  };
})();
