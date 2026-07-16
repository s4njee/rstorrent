/**
 * Renders whichever modal is currently open (driven by `ui.dialog`). Add new
 * dialogs here as their epics land; unbuilt kinds render nothing for now.
 */

import { useUi } from "../../store/ui";
import { RemoveDialog } from "./RemoveDialog";
import { AddTorrentDialog } from "./AddTorrentDialog";
import { AddMagnetDialog } from "./AddMagnetDialog";
import { PreferencesDialog } from "./PreferencesDialog";
import { StatisticsDialog } from "./StatisticsDialog";

export function DialogHost() {
  const dialog = useUi((s) => s.dialog);

  switch (dialog) {
    case "remove":
      return <RemoveDialog />;
    case "add-file":
      return <AddTorrentDialog />;
    case "add-magnet":
      return <AddMagnetDialog />;
    case "prefs":
      return <PreferencesDialog />;
    case "stats":
      return <StatisticsDialog />;
    default:
      return null;
  }
}
