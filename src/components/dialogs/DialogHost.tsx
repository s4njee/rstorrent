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
import { RateLimitDialog } from "./RateLimitDialog";
import { TuneNetworkDialog } from "./TuneNetworkDialog";
import { ShutdownDialog } from "./ShutdownDialog";

export function DialogHost() {
  const dialog = useUi((s) => s.dialog);
  const external = useUi((s) => s.externalAddRequest);

  switch (dialog) {
    case "remove":
      return <RemoveDialog />;
    case "add-file":
      return <AddTorrentDialog key={external?.id ?? "manual-file"} />;
    case "add-magnet":
      return <AddMagnetDialog key={external?.id ?? "manual-magnet"} />;
    case "prefs":
      return <PreferencesDialog />;
    case "stats":
      return <StatisticsDialog />;
    case "rate-limit":
      return <RateLimitDialog />;
    case "tune-network":
      return <TuneNetworkDialog />;
    case "shutdown":
      return <ShutdownDialog />;
    default:
      return null;
  }
}
