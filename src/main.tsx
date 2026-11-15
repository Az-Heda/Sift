import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "@fontsource/roboto/700.css";
import "@fontsource/audiowide/400.css";
import "@fontsource/anonymous-pro/400.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ThemeProvider } from "./theme/ThemeProvider";
import { applyTheme, loadTheme } from "./theme/theme";

async function start() {
  // Load + apply before first render to avoid a theme flash.
  const theme = await loadTheme();
  applyTheme(theme);

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <ThemeProvider initial={theme}>
        <App />
      </ThemeProvider>
    </React.StrictMode>,
  );
}

void start();
