/**
 * Renders whichever modal is currently open (driven by `ui.dialog`). Add new
 * dialogs here as their epics land; unbuilt kinds render nothing for now.
 */

import { useUi } from "../../store/ui";
import { RemoveDialog } from "./RemoveDialog";
import { RecheckDialog } from "./RecheckDialog";
import { AddModal } from "./AddModal";
import { PreferencesDialog } from "./PreferencesDialog";
import { StatisticsDialog } from "./StatisticsDialog";
import { RateLimitDialog } from "./RateLimitDialog";
import { TuneNetworkDialog } from "./TuneNetworkDialog";
import { ShutdownDialog } from "./ShutdownDialog";
import { SetLocationDialog } from "./SetLocationDialog";
import { CreateTorrentDialog } from "./CreateTorrentDialog";
import { SetLabelDialog } from "./SetLabelDialog";
import { SetTagsDialog } from "./SetTagsDialog";
import { MovesDialog } from "./MovesDialog";
import { SessionDialog } from "./SessionDialog";

export function DialogHost() {
  const dialog = useUi((s) => s.dialog);
  const external = useUi((s) => s.externalAddRequest);

  switch (dialog) {
    case "remove":
      return <RemoveDialog />;
    case "recheck":
      return <RecheckDialog />;
    case "add-file":
      return (
        <AddModal key={external?.id ?? "manual-file"} initialMode="file" />
      );
    case "add-magnet":
      return (
        <AddModal key={external?.id ?? "manual-magnet"} initialMode="magnet" />
      );
    case "create-torrent":
      return <CreateTorrentDialog />;
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
    case "set-location":
      return <SetLocationDialog />;
    case "set-label":
      return <SetLabelDialog onClose={() => useUi.getState().closeDialog()} />;
    case "set-tags":
      return <SetTagsDialog onClose={() => useUi.getState().closeDialog()} />;
    case "moves":
      return <MovesDialog />;
    case "session":
      return <SessionDialog />;
    default:
      return null;
  }
}
