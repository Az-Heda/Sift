import { useCallback, useState } from "react";
import "./App.css";
import Sidebar, { type ScreenId } from "./components/Sidebar";
import Compare from "./screens/Compare";
import Dashboard from "./screens/Dashboard";
import Duplicates from "./screens/Duplicates";
import Folders from "./screens/Folders";
import Quarantine from "./screens/Quarantine";
import Settings from "./screens/Settings";
import Storage from "./screens/Storage";
import { ReviewProvider } from "./review/ReviewProvider";

const SCREENS: Record<ScreenId, () => React.JSX.Element> = {
  dashboard: Dashboard,
  folders: Folders,
  duplicates: Duplicates,
  compare: Compare,
  storage: Storage,
  quarantine: Quarantine,
  settings: Settings,
};

function App() {
  const [active, setActive] = useState<ScreenId>("dashboard");
  const Current = SCREENS[active];
  const goCompare = useCallback(() => setActive("compare"), []);

  return (
    <ReviewProvider onCompare={goCompare}>
      <div className="app">
        <Sidebar active={active} onSelect={setActive} />
        <main className="content">
          <Current />
        </main>
      </div>
    </ReviewProvider>
  );
}

export default App;
