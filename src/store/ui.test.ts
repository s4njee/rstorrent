// Smart-filter store behaviour (C4): the save/remove rules and the guards that
// stop a saved filter from stranding the table in an unexplained empty state.
import { describe, it, expect, beforeEach } from "vitest";
import { canSaveSmartFilter, useUi } from "./ui";

/** Reset the store between tests (it is a module singleton). */
beforeEach(() => {
  localStorage.clear();
  useUi.setState({
    filter: null,
    search: "",
    smartFilters: [],
    selection: new Set(),
  });
});

describe("canSaveSmartFilter", () => {
  it("needs something to save", () => {
    expect(canSaveSmartFilter(null, "")).toBe(false);
    expect(canSaveSmartFilter(null, "   ")).toBe(false);
  });

  it("accepts a dimension filter or a search on its own", () => {
    expect(canSaveSmartFilter({ type: "label", value: "iso" }, "")).toBe(true);
    expect(canSaveSmartFilter(null, "ubuntu")).toBe(true);
  });

  it("refuses when a smart filter is already active", () => {
    // Criteria hold a single `text`, so "smart filter + extra search" has no
    // faithful representation — better to refuse than to silently drop one.
    expect(canSaveSmartFilter({ type: "smart", value: "sf1" }, "x")).toBe(
      false,
    );
  });
});

describe("saveSmartFilter", () => {
  it("captures the dimension filter and search, then activates it", () => {
    const ui = useUi.getState();
    ui.setFilter({ type: "status", value: "stalled" });
    useUi.setState({ search: "ubuntu" });
    useUi.getState().saveSmartFilter("stalled ubuntus");

    const s = useUi.getState();
    expect(s.smartFilters).toHaveLength(1);
    const saved = s.smartFilters[0];
    expect(saved.name).toBe("stalled ubuntus");
    expect(saved.status).toBe("stalled");
    expect(saved.text).toBe("ubuntu");
    expect(saved.label).toBeUndefined();
    // Activated, and the text now lives in the filter rather than the box.
    expect(s.filter).toEqual({ type: "smart", value: saved.id });
    expect(s.search).toBe("");
  });

  it("saves a search-only view", () => {
    useUi.setState({ search: "  fedora  " });
    useUi.getState().saveSmartFilter("fedora");
    const saved = useUi.getState().smartFilters[0];
    expect(saved.text).toBe("fedora");
    expect(saved.status).toBeUndefined();
  });

  it("ignores a blank name and an unsaveable view", () => {
    useUi.setState({ search: "x" });
    useUi.getState().saveSmartFilter("   ");
    expect(useUi.getState().smartFilters).toHaveLength(0);

    useUi.setState({ search: "", filter: null });
    useUi.getState().saveSmartFilter("nothing to save");
    expect(useUi.getState().smartFilters).toHaveLength(0);
  });

  it("persists across a reload", () => {
    useUi.setState({ search: "iso" });
    useUi.getState().saveSmartFilter("isos");
    // The store writes the whole view blob; the filter must survive in it.
    const raw = localStorage.getItem("rstorrent.view");
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!);
    expect(parsed.smartFilters).toHaveLength(1);
    expect(parsed.smartFilters[0].name).toBe("isos");
    expect(parsed.filter.type).toBe("smart");
  });
});

describe("removeSmartFilter", () => {
  it("removes it and clears the filter when it was active", () => {
    useUi.setState({ search: "iso" });
    useUi.getState().saveSmartFilter("isos");
    const id = useUi.getState().smartFilters[0].id;

    useUi.getState().removeSmartFilter(id);
    const s = useUi.getState();
    expect(s.smartFilters).toHaveLength(0);
    // Leaving the filter pointing at a deleted id would empty the table.
    expect(s.filter).toBeNull();
  });

  it("leaves an unrelated active filter alone", () => {
    useUi.setState({ search: "iso" });
    useUi.getState().saveSmartFilter("isos");
    const id = useUi.getState().smartFilters[0].id;
    useUi.getState().setFilter({ type: "label", value: "video" });

    useUi.getState().removeSmartFilter(id);
    expect(useUi.getState().filter).toEqual({ type: "label", value: "video" });
  });
});
