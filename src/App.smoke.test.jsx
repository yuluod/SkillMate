import React from "react";
import { render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";

import App from "./App.jsx";

// jsdom 未实现 matchMedia，App 的系统主题探测需要它。
beforeAll(() => {
  if (!window.matchMedia) {
    window.matchMedia = (query) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    });
  }
});

// App 冒烟测试：套件里其他测试都只渲染子视图，App 自身在 v0.0.17 曾因
// useCallback 依赖数组引用后声明的变量（TDZ）挂载即崩、整屏黑掉并带进了
// 发布。无 Tauri 桥时所有 IPC 都会走降级分支，外壳必须照常渲染——这个
// 测试保证 App 至少能完成一次真实挂载。
describe("App 冒烟", () => {
  it("挂载后渲染应用外壳，不因初始化崩溃黑屏", () => {
    render(<App />);
    expect(screen.getByText("SkillMate")).toBeTruthy();
    expect(document.querySelector(".sidebar")).toBeTruthy();
    expect(document.querySelector(".app")).toBeTruthy();
  });
});
