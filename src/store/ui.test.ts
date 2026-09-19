// Smart-filter store behaviour (C4): the save/remove rules and the guards that
// stop a saved filter from stranding the table in an unexplained empty state.
import { describe, it, expect, beforeEach } from "vitest";
import { canSaveSmartFilter, useUi } from "./ui";

/** Reset the store between tests (it is a module singleton). */
beforeEach(() => {
  localStorage.clear();
  useUi.setState({
    facets: {},
    search: "",
    smartFilters: [],
    selection: new Set(),
  });
});

describe("canSaveSmartFilter", () => {
  it("needs something to save", () => {
    expect(canSaveSmartFilter({}, "")).toBe(false);
    expect(canSaveSmartFilter({}, "   ")).toBe(false);
  });

  it("accepts a facet or a search on its own", () => {
    expect(canSaveSmartFilter({ label: "iso" }, "")).toBe(true);
    expect(
      canSaveSmartFilter({ status: "downloading", label: "iso" }, ""),
    ).toBe(true);
    expect(canSaveSmartFilter({}, "ubuntu")).toBe(true);
  });

  it("refuses while a saved filter is active", () => {
    // A saved filter's criteria hold one set of dimensions, so saving "that
    // filter, further narrowed" has nowhere to put the extra facet.
    expect(canSaveSmartFilter({ smart: "sf1" }, "x")).toBe(false);
  });
});

describe("saveSmartFilter", () => {
  it("captures the dimension filter and search, then activates it", () => {
    const ui = useUi.getState();
    ui.setFacet("status", "stalled");
    useUi.setState({ search: "ubuntu" });
    useUi.getState().saveSmartFilter("stalled ubuntus");

    const s = useUi.getState();
    expect(s.smartFilters).toHaveLength(1);
    const saved = s.smartFilters[0];
    expect(saved.name).toBe("stalled ubuntus");
    expect(saved.status).toBe("stalled");
    expect(saved.text).toBe("ubuntu");
    expect(saved.label).toBeUndefined();
    // Activated, and the criteria now live in the saved filter rather than the
    // live facets, so they cannot be applied twice.
    expect(s.facets).toEqual({ smart: saved.id });
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

    useUi.setState({ search: "", facets: {} });
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
    expect(parsed.facets.smart).toBe(parsed.smartFilters[0].id);
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
    // Leaving the facets pointing at a deleted id would empty the table.
    expect(s.facets.smart).toBeUndefined();
  });

  it("leaves unrelated facets alone", () => {
    useUi.setState({ search: "iso" });
    useUi.getState().saveSmartFilter("isos");
    const id = useUi.getState().smartFilters[0].id;
    // Something else is now the view; deleting the saved filter must not
    // disturb it.
    useUi.getState().setFacet("label", "video");
    useUi.getState().setFacet("smart", id);

    useUi.getState().removeSmartFilter(id);
    expect(useUi.getState().facets).toEqual({ label: "video" });
  });

  it("toggles one dimension at a time", () => {
    const ui = useUi.getState();
    ui.setFacet("status", "downloading");
    ui.setFacet("label", "iso");
    // The design's filters AND: both dimensions survive a second click.
    expect(useUi.getState().facets).toEqual({
      status: "downloading",
      label: "iso",
    });

    // Clicking the active value clears just that dimension.
    useUi.getState().setFacet("status", "downloading");
    expect(useUi.getState().facets).toEqual({ label: "iso" });

    useUi.getState().clearFacets();
    expect(useUi.getState().facets).toEqual({});
  });
});

describe("dialogs", () => {
  it("opens and closes set-location dialog", () => {
    useUi.getState().openDialog("set-location");
    expect(useUi.getState().dialog).toBe("set-location");
    useUi.getState().closeDialog();
    expect(useUi.getState().dialog).toBeNull();
  });

  it("opens and closes create-torrent dialog", () => {
    useUi.getState().openDialog("create-torrent");
    expect(useUi.getState().dialog).toBe("create-torrent");
    useUi.getState().closeDialog();
    expect(useUi.getState().dialog).toBeNull();
  });
});
