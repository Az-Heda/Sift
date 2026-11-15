import { listen } from "@tauri-apps/api/event";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  cancelScan,
  scanStatus,
  startScan,
  type ScanProgress,
  type ScanSummary,
} from "../api";

interface ScanCtx {
  running: boolean;
  progress: ScanProgress | null;
  /** Result of the most recent scan in this session. */
  summary: ScanSummary | null;
  error: string | null;
  /** Bumps whenever a scan ends, so screens know to refetch. */
  version: number;
  /** Tells screens to refetch after something other than a scan changed the data. */
  invalidate: () => void;
  start: () => Promise<void>;
  cancel: () => Promise<void>;
  dismiss: () => void;
}

const Ctx = createContext<ScanCtx | null>(null);
const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

export function ScanProvider({ children }: { children: ReactNode }) {
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let disposed = false;
    const unlisten: Array<() => void> = [];
    const track = (p: Promise<() => void>) =>
      p.then((u) => (disposed ? u() : unlisten.push(u)));

    track(listen<ScanProgress>("scan-progress", (e) => setProgress(e.payload)));
    track(
      listen<ScanSummary>("scan-finished", (e) => {
        setRunning(false);
        setProgress(null);
        setSummary(e.payload);
        setVersion((v) => v + 1);
      }),
    );
    track(
      listen<string>("scan-error", (e) => {
        setRunning(false);
        setProgress(null);
        setError(e.payload);
        setVersion((v) => v + 1);
      }),
    );

    // Pick up a scan that's already in flight (e.g. after a webview reload).
    scanStatus()
      .then((s) => {
        if (disposed) return;
        setRunning(s.running);
        setProgress(s.progress);
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlisten.forEach((u) => u());
    };
  }, []);

  const start = useCallback(async () => {
    setError(null);
    setSummary(null);
    try {
      await startScan();
      setRunning(true);
    } catch (e) {
      setError(errMsg(e));
    }
  }, []);

  const cancel = useCallback(async () => {
    try {
      await cancelScan();
    } catch (e) {
      setError(errMsg(e));
    }
  }, []);

  const dismiss = useCallback(() => {
    setError(null);
    setSummary(null);
  }, []);

  const invalidate = useCallback(() => setVersion((v) => v + 1), []);

  const value = useMemo<ScanCtx>(
    () => ({ running, progress, summary, error, version, invalidate, start, cancel, dismiss }),
    [running, progress, summary, error, version, invalidate, start, cancel, dismiss],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useScan() {
  const c = useContext(Ctx);
  if (!c) throw new Error("useScan must be used inside ScanProvider");
  return c;
}
