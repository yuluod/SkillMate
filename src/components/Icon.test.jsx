import React from "react";
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { IconContext } from "@phosphor-icons/react";
import Icon from "./Icon.jsx";

const NAMES = [
  "refresh", "plus", "skills", "assistants", "scenarios", "updates", "settings",
  "folder", "preview", "trash", "check", "lock", "sun", "moon", "monitor",
  "search", "tag", "x", "upload", "download", "clock", "box", "sparkles",
  "dashboard", "shield", "branch", "arrow", "external", "globe", "undo",
  "power", "unlink", "play", "edit", "copy", "more",
];

describe("Icon", () => {
  it("每个图标名都渲染出带路径的 svg", () => {
    for (const name of NAMES) {
      const { container, unmount } = render(<Icon name={name} size={16} />);
      const svg = container.querySelector("svg");
      expect(svg, name).toBeTruthy();
      expect(svg.querySelector("path"), name).toBeTruthy();
      unmount();
    }
  });

  it("保留 icon 类名并合并自定义类名", () => {
    const { container } = render(<Icon name="refresh" size={18} className="spin" />);
    const svg = container.querySelector("svg");
    expect(svg.getAttribute("class")).toBe("icon spin");
    expect(svg.getAttribute("width")).toBe("18");
    expect(svg.getAttribute("aria-hidden")).toBe("true");
  });

  it("未知图标名不渲染任何内容", () => {
    const { container } = render(<Icon name="nonexistent" />);
    expect(container.firstChild).toBeNull();
  });

  it("IconContext 的 weight 生效", () => {
    const { container } = render(
      <IconContext.Provider value={{ weight: "bold" }}>
        <Icon name="check" size={16} />
      </IconContext.Provider>,
    );
    const regular = render(<Icon name="check" size={16} />);
    expect(container.querySelector("svg").innerHTML).not.toBe(
      regular.container.querySelector("svg").innerHTML,
    );
  });
});
