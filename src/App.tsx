import { useState } from "react";
import "./App.css";
import Sidebar, { type ScreenId } from "./components/Sidebar";
import Compare from "./screens/Compare";
import Dashboard from "./screens/Dashboard";
import Duplicates from "./screens/Duplicates";
import Folders from "./screens/Folders";
import Settings from "./screens/Settings";

const SCREENS: Record<ScreenId, () => React.JSX.Element> = {
  dashboard: Dashboard,
  folders: Folders,
  duplicates: Duplicates,
  compare: Compare,
  settings: Settings,
};

function App() {
  const [active, setActive] = useState<ScreenId>("dashboard");
  const Current = SCREENS[active];

  return (
    <div className="app">
      <Sidebar active={active} onSelect={setActive} />
      <main className="content">
        <Current />
      </main>
    </div>
  );
}

export default App;
