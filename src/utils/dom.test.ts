// typingInField gates both the global shortcuts and paste-to-add. If it ever
// returned false for a focused text field, pasting a magnet into the filter box
// would silently add a torrent instead of typing — so pin the behaviour.
import { describe, it, expect, afterEach } from "vitest";
import { typingInField } from "./dom";

function mount<T extends HTMLElement>(el: T): T {
  document.body.appendChild(el);
  return el;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("typingInField", () => {
  it("is true for a focused input and textarea", () => {
    const input = mount(document.createElement("input"));
    input.focus();
    expect(typingInField()).toBe(true);

    const area = mount(document.createElement("textarea"));
    area.focus();
    expect(typingInField()).toBe(true);
  });

  it("is true for a contenteditable element", () => {
    const div = mount(document.createElement("div"));
    div.contentEditable = "true";
    // jsdom doesn't derive isContentEditable from the attribute.
    Object.defineProperty(div, "isContentEditable", { value: true });
    expect(typingInField(div)).toBe(true);
  });

  it("is false for a non-editable element and for nothing focused", () => {
    expect(typingInField(mount(document.createElement("div")))).toBe(false);
    expect(typingInField(null)).toBe(false);
  });

  it("is false after a field loses focus", () => {
    const input = mount(document.createElement("input"));
    input.focus();
    input.blur();
    expect(typingInField()).toBe(false);
  });
});
