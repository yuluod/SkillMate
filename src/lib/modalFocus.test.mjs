import test from "node:test";
import assert from "node:assert/strict";

import { activateModalFocus } from "./modalFocus.mjs";

class FakeDocument {
  constructor() {
    this.activeElement = null;
    this.listeners = new Map();
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) || [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  removeEventListener(type, listener) {
    this.listeners.set(type, (this.listeners.get(type) || []).filter((item) => item !== listener));
  }

  dispatch(type, event) {
    for (const listener of [...(this.listeners.get(type) || [])]) listener(event);
  }
}

function fakeElement(document, name) {
  return {
    name,
    focus() { document.activeElement = this; },
    hasAttribute() { return false; },
    getAttribute() { return null; },
  };
}

function fakeModal(document, name, focusable) {
  const modal = fakeElement(document, name);
  const children = new Set(focusable);
  modal.querySelectorAll = () => focusable;
  modal.contains = (element) => element === modal || children.has(element);
  return modal;
}

function keyEvent(key, shiftKey = false) {
  return {
    key,
    shiftKey,
    prevented: false,
    preventDefault() { this.prevented = true; },
  };
}

test("Modal 焦点在首尾循环、逃逸后收回，并在关闭后恢复", () => {
  const document = new FakeDocument();
  const previous = fakeElement(document, "previous");
  const first = fakeElement(document, "first");
  const last = fakeElement(document, "last");
  const outside = fakeElement(document, "outside");
  const modal = fakeModal(document, "modal", [first, last]);
  previous.focus();
  const cleanup = activateModalFocus({ document, modal, onClose() {} });

  assert.equal(document.activeElement, first);
  last.focus();
  const forward = keyEvent("Tab");
  document.dispatch("keydown", forward);
  assert.equal(document.activeElement, first);
  assert.equal(forward.prevented, true);

  first.focus();
  const backward = keyEvent("Tab", true);
  document.dispatch("keydown", backward);
  assert.equal(document.activeElement, last);

  outside.focus();
  document.dispatch("focusin", { target: outside });
  assert.equal(document.activeElement, first);

  cleanup();
  assert.equal(document.activeElement, previous);
  assert.equal(document.listeners.get("keydown").length, 0);
  assert.equal(document.listeners.get("focusin").length, 0);
});

test("堆叠 Modal 只允许最上层响应 ESC", () => {
  const document = new FakeDocument();
  const lowerButton = fakeElement(document, "lower-button");
  const upperButton = fakeElement(document, "upper-button");
  const lower = fakeModal(document, "lower", [lowerButton]);
  const upper = fakeModal(document, "upper", [upperButton]);
  let lowerCloseCount = 0;
  let upperCloseCount = 0;
  const cleanupLower = activateModalFocus({
    document,
    modal: lower,
    onClose: () => { lowerCloseCount += 1; },
  });
  const cleanupUpper = activateModalFocus({
    document,
    modal: upper,
    onClose: () => { upperCloseCount += 1; },
  });

  document.dispatch("keydown", keyEvent("Escape"));
  assert.equal(upperCloseCount, 1);
  assert.equal(lowerCloseCount, 0);

  cleanupUpper();
  document.dispatch("keydown", keyEvent("Escape"));
  assert.equal(lowerCloseCount, 1);
  cleanupLower();
});
